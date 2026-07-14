//! Presentation-optional render path: draw crusty-gui frames into an
//! externally-owned image instead of a swapchain.
//!
//! [`TargetRenderer`] is the embedding seam for engines that own their own
//! window, swapchain and frame graph (the counterpart of the shell's
//! [`WindowRenderer`](crate::window::WindowRenderer)): the host runs a
//! [`Context`](crate::context::Context) frame, hands the resulting paint list
//! plus a target [`ImageView`] here, and gets back a primary command buffer
//! to schedule wherever its frame graph wants the UI composited.
//!
//! Synchronization contract: command buffers returned by
//! [`render`](TargetRenderer::render) must be executed in submission order
//! (each frame's buffer after the previous one has finished). The glyph
//! atlas and blur targets are shared across frames, so two of this
//! renderer's command buffers must never be in flight simultaneously. A
//! typical engine frame chain (`prev_future → then_execute(ui_cb)`) already
//! guarantees this.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer, RenderPassBeginInfo,
    SubpassBeginInfo, SubpassContents,
};
use vulkano::device::{Device, Queue};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};

use crate::paint::{Mesh, PaintCmd, Tessellator};
use crate::text::TextRenderer;

use super::vulkano::{BlurRenderer, OffscreenTargets, TextureRegistry, UiBuffers, UiRenderer};

/// Renders UI paint lists into caller-owned images, one command buffer per
/// frame. Owns per-target GPU resources (texture registry, blur targets,
/// tessellator) but no window, no surface and no swapchain — presentation
/// belongs to the host. The [`TextRenderer`] is host-owned and borrowed per
/// [`render`](Self::render) call, because UI building (glyph shaping)
/// typically happens on the host's main thread while command recording
/// happens on its render thread.
pub struct TargetRenderer {
    device: Arc<Device>,
    queue: Arc<Queue>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    /// Composites over whatever the target already contains (`load_op: Load`).
    render_pass: Arc<RenderPass>,
    /// Clears the target first (`load_op: Clear`) — for hosts giving us a
    /// fresh image rather than one with a scene already in it.
    clear_render_pass: Arc<RenderPass>,
    /// Framebuffers cached per (target image, clear-variant). Cleared via
    /// [`clear_target_cache`](Self::clear_target_cache) when the host
    /// recreates its images (swapchain resize).
    framebuffers: HashMap<(usize, bool), Arc<Framebuffer>>,
    offscreen: OffscreenTargets,
    ui: UiRenderer,
    blur: BlurRenderer,
    registry: TextureRegistry,
    tessellator: Tessellator,
    ui_mesh: Mesh,
    /// Hash of the paint list `ui_mesh`/`ui_buffers` were built from; when
    /// the next frame hashes the same, tessellation and upload are skipped.
    ui_paint_hash: Option<u64>,
    ui_buffers: Option<UiBuffers>,
    tess_us: f32,
}

impl TargetRenderer {
    /// `format` must match the images later passed to [`render`](Self::render).
    pub fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        cmd_alloc: Arc<StandardCommandBufferAllocator>,
        format: Format,
    ) -> Self {
        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: format,
                    samples: 1,
                    load_op: Load,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("failed to create target render pass");

        let clear_render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("failed to create clearing target render pass");

        // Starts at 1×1; the first `render` resizes it to the real target.
        let offscreen = OffscreenTargets::new(device.clone(), format, [1, 1]);

        let ui = UiRenderer::new(
            device.clone(),
            Subpass::from(render_pass.clone(), 0).expect("render pass has subpass 0"),
        );
        let blur = BlurRenderer::new(
            device.clone(),
            Subpass::from(offscreen.render_pass().clone(), 0)
                .expect("offscreen render pass has subpass 0"),
        );
        let registry = TextureRegistry::new(device.clone());

        Self {
            device,
            queue,
            cmd_alloc,
            render_pass,
            clear_render_pass,
            framebuffers: HashMap::new(),
            offscreen,
            ui,
            blur,
            registry,
            tessellator: Tessellator::new(),
            ui_mesh: Mesh::default(),
            ui_paint_hash: None,
            ui_buffers: None,
            tess_us: 0.0,
        }
    }

    /// Mutable access to the texture registry — register the host's own
    /// image views (a viewport render target, thumbnails) to draw them with
    /// the `Image` widget, and `update` them when the host recreates them.
    pub fn registry_mut(&mut self) -> &mut TextureRegistry {
        &mut self.registry
    }

    /// Smoothed tessellation time of the last frame, in microseconds.
    pub fn tess_us(&self) -> f32 {
        self.tess_us
    }

    /// Drop cached framebuffers. Call when the host recreates the images it
    /// renders into (swapchain rebuild) so stale entries don't pin them.
    pub fn clear_target_cache(&mut self) {
        self.framebuffers.clear();
    }

    /// Record one UI frame into `target` and return the command buffer; the
    /// host schedules it in its own frame graph (see the module docs for the
    /// ordering contract).
    ///
    /// * `text` — the host-owned glyph shaper/atlas that was used to build
    ///   this frame's `Ui` (queued glyph uploads are flushed here). It must
    ///   be the same instance every frame.
    /// * `backdrop` — image the glass panels blur and sample as what is
    ///   "behind" the UI, usually the host's scene image for this frame. It
    ///   must not be `target` itself, and is sampled stretched to the full
    ///   target. `None` disables the glass effect (panels stay translucent
    ///   over a black backdrop).
    /// * `clear` — clear the target to this colour first instead of
    ///   compositing over its existing content.
    ///
    /// Returns `None` when the target has a zero extent.
    pub fn render(
        &mut self,
        text: &mut TextRenderer,
        target: Arc<ImageView>,
        ui_paint: &[PaintCmd],
        backdrop: Option<Arc<ImageView>>,
        clear: Option<[f32; 4]>,
    ) -> Option<Arc<PrimaryAutoCommandBuffer>> {
        let extent3 = target.image().extent();
        let extent = [extent3[0], extent3[1]];
        if extent[0] == 0 || extent[1] == 0 {
            return None;
        }
        self.offscreen.recreate(extent);

        let mut builder = AutoCommandBufferBuilder::primary(
            self.cmd_alloc.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .expect("failed to allocate command buffer");

        // Flush any glyph bitmaps queued by widget code.
        text.atlas_mut().flush_uploads(&mut builder);

        // Re-tessellate and re-upload only when the paint actually changed
        // (the hash is over content, so this also covers host-side mutation
        // like `scale_alpha`). The cached mesh is resolution-independent:
        // positions are in points and the screen size is a push constant.
        let paint_hash = crate::paint::paint_hash(ui_paint);
        if self.ui_paint_hash != Some(paint_hash) {
            self.ui_paint_hash = Some(paint_hash);
            let tess_start = Instant::now();
            self.tessellator.reset(&mut self.ui_mesh);
            self.tessellator.tessellate(ui_paint, &mut self.ui_mesh);
            self.ui_buffers = self.ui.upload(&self.ui_mesh);
            let tess_us = tess_start.elapsed().as_secs_f32() * 1e6;
            self.tess_us = self.tess_us * 0.9 + tess_us * 0.1;
        } else {
            self.tess_us *= 0.9;
        }

        let screen_size = [extent[0] as f32, extent[1] as f32];
        let zero_clear = [0.0_f32, 0.0, 0.0, 1.0];

        // ── Blur the backdrop (two Kawase passes) for the glass panels.
        if let Some(backdrop) = &backdrop {
            for (fbo, src, spread) in [
                (self.offscreen.blur_a_fbo().clone(), backdrop.clone(), 1.5),
                (
                    self.offscreen.blur_b_fbo().clone(),
                    self.offscreen.blur_a_view().clone(),
                    3.5,
                ),
            ] {
                builder
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            clear_values: vec![Some(zero_clear.into())],
                            ..RenderPassBeginInfo::framebuffer(fbo)
                        },
                        SubpassBeginInfo {
                            contents: SubpassContents::Inline,
                            ..Default::default()
                        },
                    )
                    .expect("begin blur render pass failed");
                self.blur.record_pass(
                    &mut builder,
                    src,
                    self.offscreen.sampler().clone(),
                    self.offscreen.extent(),
                    spread,
                );
                builder
                    .end_render_pass(Default::default())
                    .expect("end blur render pass failed");
            }
        }

        // ── UI onto the target.
        let framebuffer = self.target_framebuffer(&target, clear.is_some());
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![clear.map(Into::into)],
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("begin target render pass failed");
        let glass_backdrop = if backdrop.is_some() {
            self.offscreen.blur_b_view().clone()
        } else {
            self.offscreen.dummy_view().clone()
        };
        if let Some(buffers) = &self.ui_buffers {
            self.ui.draw(
                &mut builder,
                &self.ui_mesh,
                buffers,
                screen_size,
                glass_backdrop,
                self.offscreen.sampler().clone(),
                text.atlas().view().clone(),
                text.atlas().sampler().clone(),
                text.atlas().rgba_view().cloned(),
                Some(text.atlas().sampler().clone()),
                &self.registry,
            );
        }
        builder
            .end_render_pass(Default::default())
            .expect("end target render pass failed");

        Some(builder.build().expect("failed to build command buffer"))
    }

    fn target_framebuffer(&mut self, target: &Arc<ImageView>, clear: bool) -> Arc<Framebuffer> {
        let key = (Arc::as_ptr(target.image()) as usize, clear);
        let pass = if clear {
            &self.clear_render_pass
        } else {
            &self.render_pass
        };
        self.framebuffers
            .entry(key)
            .or_insert_with(|| {
                Framebuffer::new(
                    pass.clone(),
                    FramebufferCreateInfo {
                        attachments: vec![target.clone()],
                        ..Default::default()
                    },
                )
                .expect("failed to create target framebuffer")
            })
            .clone()
    }
}

// `device` is unused today but pins the renderer to the device its pipelines
// were built for; embedding hosts pass views from arbitrary code paths.
impl TargetRenderer {
    /// The device this renderer's pipelines were created on. Every image
    /// passed to [`render`](Self::render) must come from it.
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }
}
