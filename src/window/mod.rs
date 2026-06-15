//! winit + vulkano windowing layer.
//!
//! [`GpuContext`] owns the device-level resources shared across every
//! window (instance, logical device, queue, command-buffer allocator).
//! [`WindowRenderer`] owns the per-window surface, swapchain, offscreen
//! blur targets, and the glass/UI render pipeline — one per OS window.
//!
//! This is the reusable core behind the demo's main window and any
//! secondary windows (e.g. a mesh editor), each driven by its own
//! [`Context`](crate::context::Context).

#![cfg(all(feature = "vulkano-renderer", feature = "winit-window"))]

use std::sync::Arc;
use std::time::Instant;

use crate::backend::vulkano::{BlurRenderer, OffscreenTargets, TextureRegistry, UiRenderer};
use crate::paint::{Mesh, PaintCmd, Tessellator};
use crate::text::TextRenderer;

use vulkano::VulkanLibrary;
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, RenderPassBeginInfo, SubpassBeginInfo,
    SubpassContents,
};
use vulkano::device::physical::PhysicalDeviceType;
use vulkano::device::{
    Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
};
use vulkano::image::ImageUsage;
use vulkano::image::view::ImageView;
use vulkano::instance::{Instance, InstanceCreateFlags, InstanceCreateInfo};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::swapchain::{
    PresentMode, Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
    acquire_next_image,
};
use vulkano::sync::{self, GpuFuture};
use vulkano::{Validated, VulkanError};

use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

/// Device-level GPU resources shared by every window. Cheap to clone — all
/// fields are `Arc`s — so secondary windows reuse the same device and queue
/// rather than spinning up their own.
#[derive(Clone)]
pub struct GpuContext {
    pub instance: Arc<Instance>,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub cmd_alloc: Arc<StandardCommandBufferAllocator>,
    pub gpu_name: String,
}

impl GpuContext {
    /// Build the instance and pick a device that can present to `surface`.
    /// The surface comes from the first window so we can validate present
    /// support; the resulting context then drives any number of windows.
    fn new(instance: Arc<Instance>, surface: &Arc<Surface>) -> Self {
        let device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..DeviceExtensions::empty()
        };

        let (physical, queue_family_index) = instance
            .enumerate_physical_devices()
            .expect("could not enumerate physical devices")
            .filter(|p| p.supported_extensions().contains(&device_extensions))
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .enumerate()
                    .position(|(i, q)| {
                        q.queue_flags.intersects(QueueFlags::GRAPHICS)
                            && p.surface_support(i as u32, surface).unwrap_or(false)
                    })
                    .map(|i| (p, i as u32))
            })
            .min_by_key(|(p, _)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                _ => 4,
            })
            .expect("no graphics-capable device with present support");

        log::info!(
            "using physical device: {} ({:?})",
            physical.properties().device_name,
            physical.properties().device_type
        );

        let (device, mut queues) = Device::new(
            physical.clone(),
            DeviceCreateInfo {
                enabled_extensions: device_extensions,
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .expect("failed to create logical device");

        let queue = queues.next().expect("no queue available");
        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            StandardCommandBufferAllocatorCreateInfo::default(),
        ));
        let gpu_name = device.physical_device().properties().device_name.clone();

        Self {
            instance,
            device,
            queue,
            cmd_alloc,
            gpu_name,
        }
    }
}

/// Per-window renderer: surface, swapchain, the blur offscreen targets, and
/// the UI/glass pipeline. Each window owns its own [`TextRenderer`] glyph
/// atlas and [`TextureRegistry`], so windows are fully independent at the
/// cost of not sharing glyph/texture memory.
pub struct WindowRenderer {
    window: Arc<Window>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    swapchain: Arc<Swapchain>,
    render_pass: Arc<RenderPass>,
    framebuffers: Vec<Arc<Framebuffer>>,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    needs_swapchain_rebuild: bool,
    offscreen: OffscreenTargets,
    ui: UiRenderer,
    blur: BlurRenderer,
    text: TextRenderer,
    registry: TextureRegistry,
    tessellator: Tessellator,
    scene_mesh: Mesh,
    ui_mesh: Mesh,
    tess_us: f32,
}

impl WindowRenderer {
    /// Create the very first window and the [`GpuContext`] together: the
    /// device is chosen against this window's surface, then reused for any
    /// later windows opened with [`WindowRenderer::new`].
    pub fn create_primary(
        event_loop: &ActiveEventLoop,
        attrs: WindowAttributes,
    ) -> (GpuContext, Self) {
        let library = VulkanLibrary::new().expect("no local vulkan library / loader");
        let required_extensions = Surface::required_extensions(event_loop)
            .expect("could not query required winit surface extensions");

        let instance = Instance::new(
            library,
            InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                enabled_extensions: required_extensions,
                ..Default::default()
            },
        )
        .expect("failed to create Vulkan instance");

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let surface = Surface::from_window(instance.clone(), window.clone())
            .expect("failed to create vulkan surface from window");

        let gpu = GpuContext::new(instance, &surface);
        let renderer = Self::build(&gpu, window, surface);
        (gpu, renderer)
    }

    /// Open an additional window sharing `gpu`'s device and queue.
    pub fn new(gpu: &GpuContext, window: Arc<Window>) -> Self {
        let surface = Surface::from_window(gpu.instance.clone(), window.clone())
            .expect("failed to create vulkan surface from window");
        Self::build(gpu, window, surface)
    }

    fn build(gpu: &GpuContext, window: Arc<Window>, surface: Arc<Surface>) -> Self {
        let device = gpu.device.clone();
        let queue = gpu.queue.clone();

        let (swapchain, images) = {
            let surface_caps = device
                .physical_device()
                .surface_capabilities(&surface, Default::default())
                .expect("failed to get surface capabilities");

            let image_format = device
                .physical_device()
                .surface_formats(&surface, Default::default())
                .expect("failed to get surface formats")[0]
                .0;

            let inner_size: [u32; 2] = window.inner_size().into();

            // Prefer Mailbox (low-latency vsync) so dragging stays smooth on
            // high-refresh displays; fall back to Fifo, always available.
            let present_modes: Vec<PresentMode> = device
                .physical_device()
                .surface_present_modes(&surface, Default::default())
                .unwrap_or_default();
            let present_mode = if present_modes.contains(&PresentMode::Mailbox) {
                PresentMode::Mailbox
            } else {
                PresentMode::Fifo
            };
            let mut min_image_count = if present_mode == PresentMode::Mailbox {
                surface_caps.min_image_count.max(3)
            } else {
                surface_caps.min_image_count.max(2)
            };
            if let Some(max) = surface_caps.max_image_count {
                min_image_count = min_image_count.min(max);
            }

            Swapchain::new(
                device.clone(),
                surface.clone(),
                SwapchainCreateInfo {
                    min_image_count,
                    image_format,
                    image_extent: inner_size,
                    image_usage: ImageUsage::COLOR_ATTACHMENT,
                    composite_alpha: surface_caps
                        .supported_composite_alpha
                        .into_iter()
                        .next()
                        .expect("no supported composite alpha"),
                    present_mode,
                    ..Default::default()
                },
            )
            .expect("failed to create swapchain")
        };

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: swapchain.image_format(),
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
        .expect("failed to create render pass");

        let framebuffers = build_framebuffers(&images, &render_pass);

        let previous_frame_end = Some(sync::now(device.clone()).boxed());

        let surface_format = swapchain.image_format();
        let inner_size: [u32; 2] = window.inner_size().into();
        let offscreen = OffscreenTargets::new(device.clone(), surface_format, inner_size);

        let ui = UiRenderer::new(
            device.clone(),
            Subpass::from(render_pass.clone(), 0).expect("render pass has subpass 0"),
        );
        let blur = BlurRenderer::new(
            device.clone(),
            Subpass::from(offscreen.render_pass().clone(), 0)
                .expect("offscreen render pass has subpass 0"),
        );
        let text = TextRenderer::new(device.clone(), [1024, 1024]);
        let registry = TextureRegistry::new(device.clone());

        Self {
            window,
            device,
            queue,
            cmd_alloc: gpu.cmd_alloc.clone(),
            swapchain,
            render_pass,
            framebuffers,
            previous_frame_end,
            needs_swapchain_rebuild: false,
            offscreen,
            ui,
            blur,
            text,
            registry,
            tessellator: Tessellator::new(),
            scene_mesh: Mesh::default(),
            ui_mesh: Mesh::default(),
            tess_us: 0.0,
        }
    }

    /// The winit window this renderer presents to.
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Mutable access to this window's text renderer (glyph shaping + atlas).
    pub fn text_mut(&mut self) -> &mut TextRenderer {
        &mut self.text
    }

    /// Mutable access to this window's texture registry.
    pub fn registry_mut(&mut self) -> &mut TextureRegistry {
        &mut self.registry
    }

    /// Smoothed tessellation time of the last frame, in microseconds.
    pub fn tess_us(&self) -> f32 {
        self.tess_us
    }

    /// Mark the swapchain stale (e.g. after a resize); it is rebuilt on the
    /// next [`WindowRenderer::render`].
    pub fn request_rebuild(&mut self) {
        self.needs_swapchain_rebuild = true;
    }

    fn recreate_swapchain(&mut self) {
        let inner_size: [u32; 2] = self.window.inner_size().into();
        if inner_size[0] == 0 || inner_size[1] == 0 {
            return;
        }
        let (new_sc, new_images) = self
            .swapchain
            .recreate(SwapchainCreateInfo {
                image_extent: inner_size,
                ..self.swapchain.create_info()
            })
            .expect("failed to recreate swapchain");
        self.swapchain = new_sc;
        self.framebuffers = build_framebuffers(&new_images, &self.render_pass);
        self.offscreen.recreate(inner_size);
        self.needs_swapchain_rebuild = false;
    }

    /// Tessellate `scene_paint` (background) and `ui_paint` (foreground) and
    /// present a frame: scene → offscreen, two Kawase blur passes, then the
    /// glass blit + UI overlay onto the swapchain image.
    pub fn render(&mut self, scene_paint: &[PaintCmd], ui_paint: &[PaintCmd]) {
        let inner_size: [u32; 2] = self.window.inner_size().into();
        if inner_size[0] == 0 || inner_size[1] == 0 {
            return;
        }

        if self.needs_swapchain_rebuild {
            self.recreate_swapchain();
        }

        // The glyph atlas is one image shared across frames; if new glyphs
        // were queued we must finish in-flight frames still sampling it
        // before overwriting its pixels. Stalls only on upload frames.
        if self.text.atlas().has_pending() {
            let _ = unsafe { self.device.wait_idle() };
            self.previous_frame_end = Some(sync::now(self.device.clone()).boxed());
        }

        self.previous_frame_end.as_mut().unwrap().cleanup_finished();

        let (image_index, suboptimal, acquire_future) =
            match acquire_next_image(self.swapchain.clone(), None).map_err(Validated::unwrap) {
                Ok(r) => r,
                Err(VulkanError::OutOfDate) => {
                    self.needs_swapchain_rebuild = true;
                    return;
                }
                Err(e) => panic!("acquire_next_image failed: {e}"),
            };

        if suboptimal {
            self.needs_swapchain_rebuild = true;
        }

        let mut builder = AutoCommandBufferBuilder::primary(
            self.cmd_alloc.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .expect("failed to allocate command buffer");

        // Flush any glyph bitmaps queued by widget code.
        self.text.atlas_mut().flush_uploads(&mut builder);

        let tess_start = Instant::now();
        self.tessellator.reset(&mut self.scene_mesh);
        self.tessellator.tessellate(scene_paint, &mut self.scene_mesh);
        self.tessellator.reset(&mut self.ui_mesh);
        self.tessellator.tessellate(ui_paint, &mut self.ui_mesh);
        let tess_us = tess_start.elapsed().as_secs_f32() * 1e6;
        self.tess_us = self.tess_us * 0.9 + tess_us * 0.1;

        let screen_size = [inner_size[0] as f32, inner_size[1] as f32];
        let bg_clear = [0.04_f32, 0.05, 0.08, 1.0];
        let zero_clear = [0.0_f32, 0.0, 0.0, 1.0];

        // ── Pass 1: scene → offscreen scene image.
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(bg_clear.into())],
                    ..RenderPassBeginInfo::framebuffer(self.offscreen.scene_fbo().clone())
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("begin scene render pass failed");
        self.ui.draw(
            &mut builder,
            &self.scene_mesh,
            screen_size,
            self.offscreen.dummy_view().clone(),
            self.offscreen.sampler().clone(),
            self.text.atlas().view().clone(),
            self.text.atlas().sampler().clone(),
            self.text.atlas().rgba_view().cloned(),
            Some(self.text.atlas().sampler().clone()),
            &self.registry,
        );
        builder
            .end_render_pass(Default::default())
            .expect("end scene render pass failed");

        // ── Pass 2: blur scene → blur_a.
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(zero_clear.into())],
                    ..RenderPassBeginInfo::framebuffer(self.offscreen.blur_a_fbo().clone())
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("begin blur-a render pass failed");
        self.blur.record_pass(
            &mut builder,
            self.offscreen.scene_view().clone(),
            self.offscreen.sampler().clone(),
            self.offscreen.extent(),
            1.5,
        );
        builder
            .end_render_pass(Default::default())
            .expect("end blur-a render pass failed");

        // ── Pass 3: blur blur_a → blur_b.
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(zero_clear.into())],
                    ..RenderPassBeginInfo::framebuffer(self.offscreen.blur_b_fbo().clone())
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("begin blur-b render pass failed");
        self.blur.record_pass(
            &mut builder,
            self.offscreen.blur_a_view().clone(),
            self.offscreen.sampler().clone(),
            self.offscreen.extent(),
            3.5,
        );
        builder
            .end_render_pass(Default::default())
            .expect("end blur-b render pass failed");

        // ── Pass 4: present. Blit scene as background, then UI.
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(bg_clear.into())],
                    ..RenderPassBeginInfo::framebuffer(
                        self.framebuffers[image_index as usize].clone(),
                    )
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("begin present render pass failed");
        self.blur.record_pass(
            &mut builder,
            self.offscreen.scene_view().clone(),
            self.offscreen.sampler().clone(),
            inner_size,
            0.0,
        );
        self.ui.draw(
            &mut builder,
            &self.ui_mesh,
            screen_size,
            self.offscreen.blur_b_view().clone(),
            self.offscreen.sampler().clone(),
            self.text.atlas().view().clone(),
            self.text.atlas().sampler().clone(),
            self.text.atlas().rgba_view().cloned(),
            Some(self.text.atlas().sampler().clone()),
            &self.registry,
        );
        builder
            .end_render_pass(Default::default())
            .expect("end present render pass failed");

        let command_buffer = builder.build().expect("failed to build command buffer");

        let future = self
            .previous_frame_end
            .take()
            .unwrap()
            .join(acquire_future)
            .then_execute(self.queue.clone(), command_buffer)
            .expect("failed to chain command buffer execution")
            .then_swapchain_present(
                self.queue.clone(),
                SwapchainPresentInfo::swapchain_image_index(self.swapchain.clone(), image_index),
            )
            .then_signal_fence_and_flush();

        match future.map_err(Validated::unwrap) {
            Ok(f) => self.previous_frame_end = Some(f.boxed()),
            Err(VulkanError::OutOfDate) => {
                self.needs_swapchain_rebuild = true;
                self.previous_frame_end = Some(sync::now(self.device.clone()).boxed());
            }
            Err(e) => {
                log::error!("present failed: {e}");
                self.previous_frame_end = Some(sync::now(self.device.clone()).boxed());
            }
        }
    }
}

fn build_framebuffers(
    images: &[Arc<vulkano::image::Image>],
    render_pass: &Arc<RenderPass>,
) -> Vec<Arc<Framebuffer>> {
    images
        .iter()
        .map(|image| {
            let view = ImageView::new_default(image.clone()).expect("image view");
            Framebuffer::new(
                render_pass.clone(),
                FramebufferCreateInfo {
                    attachments: vec![view],
                    ..Default::default()
                },
            )
            .expect("framebuffer")
        })
        .collect()
}
