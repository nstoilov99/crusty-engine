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
        }
    }

    pub fn view(&self) -> &Arc<ImageView> {
        &self.view
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

    /// Copy every queued upload into the atlas image. Call once per frame,
    /// before any render pass that samples the atlas.
    pub fn flush_uploads(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        if self.pending.is_empty() {
            return;
        }
        // One staging buffer per upload — small (a glyph bitmap is at most
        // a few kB), allocations are cheap; coalescing into a single
        // staging buffer is a future optimisation.
        for upload in self.pending.drain(..) {
            let staging = Buffer::from_iter(
                self.memory_allocator.clone(),
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
                    aspects: self.image.format().aspects(),
                    mip_level: 0,
                    array_layers: 0..1,
                },
                image_offset: [upload.x, upload.y, 0],
                image_extent: [upload.w, upload.h, 1],
                ..Default::default()
            };
            let copy_info = CopyBufferToImageInfo {
                regions: smallvec::smallvec![region],
                ..CopyBufferToImageInfo::buffer_image(staging, self.image.clone())
            };
            builder
                .copy_buffer_to_image(copy_info)
                .expect("copy_buffer_to_image failed");
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
