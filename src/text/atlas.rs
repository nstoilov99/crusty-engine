//! GPU glyph atlas. One large R8 image holds every rasterized glyph used
//! by the live UI; [`TextRenderer`](super::TextRenderer) hands the atlas
//! cache keys + bitmap bytes and gets back UV rectangles to embed in
//! `PaintCmd::Glyph` commands.
//!
//! Updates are batched: every glyph the renderer pushes is queued into a
//! pending list, and the host calls [`GlyphAtlas::flush_uploads`] once per
//! frame (outside any render pass) to copy them all into the image at
//! once.

use std::sync::Arc;

use etagere::{AtlasAllocator, Size as EtagereSize};
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BufferImageCopy, CopyBufferToImageInfo, PrimaryAutoCommandBuffer,
};
use vulkano::format::Format;
use vulkano::image::sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageSubresourceLayers, ImageType, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::DeviceSize;

use crate::math::Pos2;

/// Atlas-side handle to a packed glyph: pixel rectangle + precomputed UV
/// rectangle ready to drop into a `PaintCmd::Glyph`.
#[derive(Clone, Copy, Debug)]
pub struct AtlasRect {
    pub uv_min: Pos2,
    pub uv_max: Pos2,
    /// Pixel position+size inside the atlas. Currently only used for
    /// diagnostics, but worth keeping around for tooling.
    pub px_x: u32,
    pub px_y: u32,
    pub px_w: u32,
    pub px_h: u32,
}

pub struct GlyphAtlas {
    memory_allocator: Arc<StandardMemoryAllocator>,
    image: Arc<Image>,
    view: Arc<ImageView>,
    sampler: Arc<Sampler>,
    extent: [u32; 2],
    allocator: AtlasAllocator,
    pending: Vec<PendingUpload>,

    // Second page: RGBA8, for color glyphs (emoji). Allocated lazily on the
    // first color glyph so non-emoji UIs pay nothing.
    rgba_image: Option<Arc<Image>>,
    rgba_view: Option<Arc<ImageView>>,
    rgba_allocator: Option<AtlasAllocator>,
    rgba_pending: Vec<PendingUpload>,
}

struct PendingUpload {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    bytes: Vec<u8>,
}

impl GlyphAtlas {
    pub fn new(
        memory_allocator: Arc<StandardMemoryAllocator>,
        device: Arc<vulkano::device::Device>,
        extent: [u32; 2],
    ) -> Self {
        let image = Image::new(
            memory_allocator.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: Format::R8_UNORM,
                extent: [extent[0], extent[1], 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo::default(),
        )
        .expect("failed to allocate glyph atlas image");
        let view = ImageView::new_default(image.clone()).expect("failed to create atlas view");

        let sampler = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("failed to create atlas sampler");

        let allocator =
            AtlasAllocator::new(EtagereSize::new(extent[0] as i32, extent[1] as i32));

        Self {
            memory_allocator,
            image,
            view,
            sampler,
            extent,
            allocator,
            pending: Vec::new(),
            rgba_image: None,
            rgba_view: None,
            rgba_allocator: None,
            rgba_pending: Vec::new(),
        }
    }

    pub fn view(&self) -> &Arc<ImageView> {
        &self.view
    }

    /// The RGBA color-glyph page view, if it has been allocated. `None`
    /// until the first color glyph is inserted.
    pub fn rgba_view(&self) -> Option<&Arc<ImageView>> {
        self.rgba_view.as_ref()
    }

    /// Whether any glyph bitmaps (mask or color) are queued for upload.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.rgba_pending.is_empty()
    }

    pub fn sampler(&self) -> &Arc<Sampler> {
        &self.sampler
    }

    /// Allocate a region of `(w, h)` pixels and queue `bytes` to be copied
    /// into it. Returns `None` if the atlas is full — caller should treat
    /// that as "skip this glyph".
    pub fn insert(&mut self, w: u32, h: u32, bytes: Vec<u8>) -> Option<AtlasRect> {
        debug_assert_eq!(bytes.len(), (w * h) as usize, "bitmap size mismatch");
        // etagere refuses zero-sized allocations.
        let alloc_size = EtagereSize::new(w.max(1) as i32, h.max(1) as i32);
        let allocation = self.allocator.allocate(alloc_size)?;
        let r = allocation.rectangle;
        let px_x = r.min.x as u32;
        let px_y = r.min.y as u32;
        self.pending.push(PendingUpload {
            x: px_x,
            y: px_y,
            w,
            h,
            bytes,
        });
        let inv_w = 1.0 / self.extent[0] as f32;
        let inv_h = 1.0 / self.extent[1] as f32;
        Some(AtlasRect {
            uv_min: Pos2::new(px_x as f32 * inv_w, px_y as f32 * inv_h),
            uv_max: Pos2::new((px_x + w) as f32 * inv_w, (px_y + h) as f32 * inv_h),
            px_x,
            px_y,
            px_w: w,
            px_h: h,
        })
    }

    /// Allocate an RGBA region of `(w, h)` pixels and queue `bytes`
    /// (`w * h * 4` straight-alpha RGBA) to be copied into the color page.
    /// Lazily creates the page on first use. Returns `None` if full.
    pub fn insert_rgba(&mut self, w: u32, h: u32, bytes: Vec<u8>) -> Option<AtlasRect> {
        debug_assert_eq!(bytes.len(), (w * h * 4) as usize, "rgba bitmap size mismatch");
        self.ensure_rgba_page();
        let allocator = self.rgba_allocator.as_mut()?;
        let alloc_size = EtagereSize::new(w.max(1) as i32, h.max(1) as i32);
        let allocation = allocator.allocate(alloc_size)?;
        let r = allocation.rectangle;
        let px_x = r.min.x as u32;
        let px_y = r.min.y as u32;
        self.rgba_pending.push(PendingUpload {
            x: px_x,
            y: px_y,
            w,
            h,
            bytes,
        });
        let inv_w = 1.0 / self.extent[0] as f32;
        let inv_h = 1.0 / self.extent[1] as f32;
        Some(AtlasRect {
            uv_min: Pos2::new(px_x as f32 * inv_w, px_y as f32 * inv_h),
            uv_max: Pos2::new((px_x + w) as f32 * inv_w, (px_y + h) as f32 * inv_h),
            px_x,
            px_y,
            px_w: w,
            px_h: h,
        })
    }

    fn ensure_rgba_page(&mut self) {
        if self.rgba_image.is_some() {
            return;
        }
        let image = Image::new(
            self.memory_allocator.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: Format::R8G8B8A8_UNORM,
                extent: [self.extent[0], self.extent[1], 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo::default(),
        )
        .expect("failed to allocate rgba glyph atlas image");
        let view = ImageView::new_default(image.clone()).expect("failed to create rgba atlas view");
        self.rgba_image = Some(image);
        self.rgba_view = Some(view);
        self.rgba_allocator = Some(AtlasAllocator::new(EtagereSize::new(
            self.extent[0] as i32,
            self.extent[1] as i32,
        )));
    }

    /// Copy every queued upload into the atlas images. Call once per frame,
    /// before any render pass that samples the atlas.
    pub fn flush_uploads(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        let r8_image = self.image.clone();
        flush_page(
            &self.memory_allocator,
            builder,
            &mut self.pending,
            r8_image,
        );
        if let Some(rgba_image) = self.rgba_image.clone() {
            flush_page(
                &self.memory_allocator,
                builder,
                &mut self.rgba_pending,
                rgba_image,
            );
        }
    }

    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }

    /// Bytes per row, accounting for tight packing (R8 = 1 byte/pixel).
    pub fn row_pitch(&self) -> DeviceSize {
        self.extent[0] as DeviceSize
    }
}

/// Drain `pending` into `image` via per-upload staging buffers. Shared by
/// the R8 and RGBA pages — the copy is format-agnostic (byte ranges).
fn flush_page(
    memory_allocator: &Arc<StandardMemoryAllocator>,
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    pending: &mut Vec<PendingUpload>,
    image: Arc<Image>,
) {
    if pending.is_empty() {
        return;
    }
    for upload in pending.drain(..) {
        let staging = Buffer::from_iter(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            upload.bytes.into_iter(),
        )
        .expect("failed to allocate glyph staging buffer");

        let region = BufferImageCopy {
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: ImageSubresourceLayers {
                aspects: image.format().aspects(),
                mip_level: 0,
                array_layers: 0..1,
            },
            image_offset: [upload.x, upload.y, 0],
            image_extent: [upload.w, upload.h, 1],
            ..Default::default()
        };
        let copy_info = CopyBufferToImageInfo {
            regions: smallvec::smallvec![region],
            ..CopyBufferToImageInfo::buffer_image(staging, image.clone())
        };
        builder
            .copy_buffer_to_image(copy_info)
            .expect("copy_buffer_to_image failed");
    }
}
