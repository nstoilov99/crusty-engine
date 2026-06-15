//! Phase 3 demo binary for crusty-gui.
//!
//! Adds interactive widgets driven by the immediate-mode `Ui` API:
//! pointer events from winit flow through `Context::begin_frame`, the
//! demo closure builds a `Label` and two `Button`s inside the glass
//! panel, and the resulting `PaintCmd` list is tessellated and rendered
//! through the same multi-pass pipeline from Phase 1.5.

use std::sync::Arc;
use std::time::Instant;

use crusty_gui::backend::vulkano::{
    BlurRenderer, OffscreenTargets, TextureRegistry, UiRenderer, upload_rgba_texture,
};
use crusty_gui::dock::{DockArea, DockNode, DockState};
use crusty_gui::paint::{Mesh, Tessellator};
use crusty_gui::prelude::*;
use crusty_gui::style::Style;
use crusty_gui::text::TextRenderer;
use crusty_gui::widgets::Window as GuiWindow;

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

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

struct Renderer {
    window: Arc<Window>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    swapchain: Arc<Swapchain>,
    render_pass: Arc<RenderPass>,
    framebuffers: Vec<Arc<Framebuffer>>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    needs_swapchain_rebuild: bool,
    offscreen: OffscreenTargets,
    ui: UiRenderer,
    blur: BlurRenderer,
    text: TextRenderer,
    registry: TextureRegistry,
    demo_texture: TextureId,
    tessellator: Tessellator,
    scene_mesh: Mesh,
    ui_mesh: Mesh,
    tess_us: f32,
    gpu_name: String,
}

impl Renderer {
    fn new(event_loop: &ActiveEventLoop) -> Self {
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
                .create_window(
                    WindowAttributes::default()
                        .with_title("crusty-gui — Phase 8 (combo + tooltip + ctx menu)")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0)),
                )
                .expect("failed to create window"),
        );

        let surface = Surface::from_window(instance.clone(), window.clone())
            .expect("failed to create vulkan surface from window");

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
                            && p.surface_support(i as u32, &surface).unwrap_or(false)
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

            // Prefer Mailbox (low-latency vsync: always presents the newest
            // queued frame, no tearing) so dragging stays smooth on high-
            // refresh displays. Fall back to Fifo, which is always available.
            let present_modes: Vec<PresentMode> = device
                .physical_device()
                .surface_present_modes(&surface, Default::default())
                .unwrap_or_default();
            let present_mode = if present_modes.contains(&PresentMode::Mailbox) {
                PresentMode::Mailbox
            } else {
                PresentMode::Fifo
            };
            // Mailbox needs a spare image to swap to; triple-buffer it.
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

        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            StandardCommandBufferAllocatorCreateInfo::default(),
        ));

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

        // A simple checkerboard texture so the Viewport panel has something
        // real to display through the Phase 10 texture path.
        let mut registry = TextureRegistry::new(device.clone());
        let demo_texture = {
            const N: u32 = 64;
            let mut pixels = Vec::with_capacity((N * N * 4) as usize);
            for y in 0..N {
                for x in 0..N {
                    let on = ((x / 8) + (y / 8)) % 2 == 0;
                    let (r, g, b) = if on { (90, 130, 200) } else { (30, 40, 60) };
                    pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
            let view = upload_rgba_texture(device.clone(), queue.clone(), N, N, &pixels);
            registry.register(view, TextureFilter::Nearest)
        };

        let gpu_name = device.physical_device().properties().device_name.clone();

        Self {
            window,
            device,
            queue,
            swapchain,
            render_pass,
            framebuffers,
            cmd_alloc,
            previous_frame_end,
            needs_swapchain_rebuild: false,
            offscreen,
            ui,
            blur,
            text,
            registry,
            demo_texture,
            tessellator: Tessellator::new(),
            scene_mesh: Mesh::default(),
            ui_mesh: Mesh::default(),
            tess_us: 0.0,
            gpu_name,
        }
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

    /// Run a UI pass against the inner [`TextRenderer`]. The closure
    /// receives a `&mut Ui` covering `rect`; widget paint commands land
    /// on `ctx.paint` directly.
    fn run_ui<R>(
        &mut self,
        ctx: &mut Context,
        rect: Rect,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        ctx.run_root(&mut self.text, rect, f)
    }

    fn render(&mut self, scene_paint: &[PaintCmd], ui_paint: &[PaintCmd]) {
        let inner_size: [u32; 2] = self.window.inner_size().into();
        if inner_size[0] == 0 || inner_size[1] == 0 {
            return;
        }

        if self.needs_swapchain_rebuild {
            self.recreate_swapchain();
        }

        // The glyph atlas is a single image shared across frames. If new
        // glyphs were queued this frame (e.g. a theme/font change), we must
        // finish any in-flight frames still sampling it before overwriting
        // its pixels — vulkano can't synchronise a write to a resource a
        // live submission is still reading. This stalls only on the rare
        // frames that actually upload new glyphs.
        if self.text.atlas().has_pending() {
            // Safe: we own every queue submission on this device and are
            // about to rebuild the frame, so nothing else is recording.
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

/// Background scene. Empty in Phase 6.5 — the editor layout fills the
/// whole window so any backdrop content just shows through translucent
/// panels and competes with drop-zone overlays. Kept as a function so we
/// can drop content back in if we want to demo glassmorphism over a
/// rendered scene again.
fn build_scene_paint(_screen: [u32; 2]) -> Vec<PaintCmd> {
    Vec::new()
}


#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

// Static id strings for demo selection. We use &'static str so the
// selection state can be `Copy`.
const NPC_IDS: [&str; 24] = [
    "npc_00", "npc_01", "npc_02", "npc_03", "npc_04", "npc_05",
    "npc_06", "npc_07", "npc_08", "npc_09", "npc_10", "npc_11",
    "npc_12", "npc_13", "npc_14", "npc_15", "npc_16", "npc_17",
    "npc_18", "npc_19", "npc_20", "npc_21", "npc_22", "npc_23",
];
const LIGHT_IDS: [&str; 8] = [
    "light_0", "light_1", "light_2", "light_3",
    "light_4", "light_5", "light_6", "light_7",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Theme {
    EditorDark,
    Glassmorphism,
}

impl Theme {
    fn style(self) -> Style {
        match self {
            Theme::EditorDark => Style::editor_dark(),
            Theme::Glassmorphism => Style::glassmorphism(),
        }
    }
}

struct App {
    renderer: Option<Renderer>,
    ctx: Context,
    start: Option<Instant>,
    last_frame_time: f64,
    pointer_pos: Option<Pos2>,
    pending_events: Vec<Event>,
    modifiers: Modifiers,
    // Demo state — persisted across frames in App, mutated through widgets.
    click_count: u32,
    volume: f32,
    brightness: f32,
    preview_enabled: bool,
    auto_save: bool,
    quality: Quality,
    theme: Theme,
    dock_tree: DockNode,
    dock_state: DockState,
    entity_name: String,
    asset_path: String,
    selected_entity: &'static str,
    settings_window_open: bool,
    about_modal_open: bool,
    window_slider: f32,
    ui_us: f32,
    /// Asset name dropped onto the Inspector's material slot (Phase 13 DnD).
    inspector_slot: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            renderer: None,
            ctx: Context::default(),
            start: None,
            last_frame_time: 0.0,
            pointer_pos: None,
            pending_events: Vec::new(),
            modifiers: Modifiers::empty(),
            click_count: 0,
            volume: 0.6,
            brightness: 0.8,
            preview_enabled: true,
            auto_save: false,
            quality: Quality::Medium,
            theme: Theme::EditorDark,
            dock_tree: default_dock_tree(),
            dock_state: DockState::default(),
            entity_name: "PlayerCamera".to_string(),
            asset_path: "assets/scenes/demo.scene".to_string(),
            selected_entity: "",
            settings_window_open: true,
            about_modal_open: false,
            window_slider: 0.4,
            ui_us: 0.0,
            inspector_slot: None,
        }
    }
}

fn default_dock_tree() -> DockNode {
    // Left | (top center over bottom console). Top center = viewport |
    // inspector. Console leaf has 3 tabs to showcase tab switching.
    DockNode::split_h(
        0.18,
        DockNode::leaf("Hierarchy"),
        DockNode::split_v(
            0.72,
            DockNode::split_h(
                0.72,
                DockNode::leaf("Viewport"),
                DockNode::leaf("Inspector"),
            ),
            DockNode::tabs(vec![
                "Console".into(),
                "Profiler".into(),
                "Errors".into(),
                "Warnings".into(),
            ]),
        ),
    )
}

/// Where the demo persists its dock layout + UI memory between runs.
const LAYOUT_PATH: &str = "crusty_layout.json";

/// Borrowed view written to disk — serializes the dock tree alongside the
/// `Context`'s `Memory` (window positions, scroll offsets, animation state)
/// so a relaunch restores both the panel arrangement and the UI state.
#[derive(serde::Serialize)]
struct LayoutRef<'a> {
    dock: &'a DockNode,
    memory: &'a crusty_gui::context::Memory,
}

/// Owned counterpart read back from disk.
#[derive(serde::Deserialize)]
struct LayoutOwned {
    dock: DockNode,
    memory: crusty_gui::context::Memory,
}

impl App {
    /// Restore the dock tree + UI memory from `LAYOUT_PATH` if it exists and
    /// parses. A missing or stale file just leaves the defaults in place.
    fn load_layout(&mut self) {
        let Ok(text) = std::fs::read_to_string(LAYOUT_PATH) else {
            return;
        };
        match serde_json::from_str::<LayoutOwned>(&text) {
            Ok(layout) => {
                self.dock_tree = layout.dock;
                self.ctx.memory = layout.memory;
                log::info!("restored dock layout from {LAYOUT_PATH}");
            }
            Err(e) => log::warn!("ignoring unreadable {LAYOUT_PATH}: {e}"),
        }
    }

    /// Write the current dock tree + UI memory to `LAYOUT_PATH`.
    fn save_layout(&self) {
        let layout = LayoutRef {
            dock: &self.dock_tree,
            memory: &self.ctx.memory,
        };
        match serde_json::to_string_pretty(&layout) {
            Ok(json) => {
                if let Err(e) = std::fs::write(LAYOUT_PATH, json) {
                    log::warn!("failed writing {LAYOUT_PATH}: {e}");
                }
            }
            Err(e) => log::warn!("failed serializing layout: {e}"),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(event_loop));
            self.start = Some(Instant::now());
            self.load_layout();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                self.save_layout();
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                renderer.needs_swapchain_rebuild = true;
                renderer.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Pos2::new(position.x as f32, position.y as f32);
                self.pointer_pos = Some(pos);
                self.pending_events.push(Event::PointerMoved(pos));
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer_pos = None;
                self.pending_events.push(Event::PointerGone);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let our_button = match button {
                    winit::event::MouseButton::Left => MouseButton::Primary,
                    winit::event::MouseButton::Right => MouseButton::Secondary,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Other(0),
                };
                let pressed = matches!(state, winit::event::ElementState::Pressed);
                if let Some(pos) = self.pointer_pos {
                    self.pending_events.push(Event::PointerButton {
                        pos,
                        button: our_button,
                        pressed,
                        modifiers: self.modifiers,
                    });
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                let s = mods.state();
                let mut m = Modifiers::empty();
                m.set(Modifiers::SHIFT, s.shift_key());
                m.set(Modifiers::CTRL, s.control_key());
                m.set(Modifiers::ALT, s.alt_key());
                m.set(Modifiers::META, s.super_key());
                self.modifiers = m;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let v = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(
                        (p.x as f32) / 20.0,
                        (p.y as f32) / 20.0,
                    ),
                };
                self.pending_events.push(Event::Scroll(v));
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::{Key as WKey, NamedKey};
                let pressed = matches!(key_event.state, ElementState::Pressed);
                let key = match &key_event.logical_key {
                    WKey::Named(named) => match named {
                        NamedKey::Backspace => Key::Backspace,
                        NamedKey::Enter => Key::Enter,
                        NamedKey::Tab => Key::Tab,
                        NamedKey::Escape => Key::Escape,
                        NamedKey::Space => Key::Space,
                        NamedKey::ArrowLeft => Key::ArrowLeft,
                        NamedKey::ArrowRight => Key::ArrowRight,
                        NamedKey::ArrowUp => Key::ArrowUp,
                        NamedKey::ArrowDown => Key::ArrowDown,
                        NamedKey::Home => Key::Home,
                        NamedKey::End => Key::End,
                        NamedKey::PageUp => Key::PageUp,
                        NamedKey::PageDown => Key::PageDown,
                        NamedKey::Delete => Key::Delete,
                        NamedKey::Insert => Key::Insert,
                        _ => Key::Unknown,
                    },
                    WKey::Character(s) => {
                        s.chars().next().map(Key::Char).unwrap_or(Key::Unknown)
                    }
                    _ => Key::Unknown,
                };
                self.pending_events.push(Event::Key {
                    key,
                    pressed,
                    repeat: key_event.repeat,
                    modifiers: self.modifiers,
                });
                if pressed {
                    if let Some(text) = &key_event.text {
                        self.pending_events
                            .push(Event::Text(text.as_str().to_string()));
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                renderer.text.new_frame();
                let size = renderer.window.inner_size();
                let pixels_per_point = renderer.window.scale_factor() as f32;
                let inner_size: [u32; 2] = size.into();
                let time = self
                    .start
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or_default();
                let dt = if self.last_frame_time > 0.0 {
                    // Floor at 1 ms so animations always advance even on
                    // back-to-back redraws within the same OS tick.
                    (time - self.last_frame_time).max(0.001).min(0.1) as f32
                } else {
                    1.0 / 60.0
                };
                self.last_frame_time = time;

                let screen_rect = Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(size.width as f32, size.height as f32),
                );

                let input = RawInput {
                    screen_rect,
                    pixels_per_point,
                    time,
                    predicted_dt: dt,
                    modifiers: self.modifiers,
                    events: std::mem::take(&mut self.pending_events),
                };

                let ui_start = Instant::now();
                self.ctx.begin_frame(input);
                // Apply the active theme before any widget reads it.
                self.ctx.style = self.theme.style();

                let mut click_delta: u32 = 0;
                let dock_root = Rect::from_min_size(
                    Pos2::new(4.0, 4.0),
                    Vec2::new(
                        (size.width as f32 - 8.0).max(64.0),
                        (size.height as f32 - 8.0).max(64.0),
                    ),
                );
                let volume = &mut self.volume;
                let brightness = &mut self.brightness;
                let preview_enabled = &mut self.preview_enabled;
                let auto_save = &mut self.auto_save;
                let quality = &mut self.quality;
                let theme = &mut self.theme;
                let entity_name = &mut self.entity_name;
                let asset_path = &mut self.asset_path;
                let inspector_slot = &mut self.inspector_slot;
                let selected_entity = &mut self.selected_entity;
                let click_count = self.click_count;
                let frame_time_ms = (dt * 1000.0) as f32;
                // Isolated CPU costs (previous frame, smoothed): the
                // immediate-mode UI pass and tessellation, in µs. These are
                // the figures comparable to egui's reported CPU time.
                let ui_us = self.ui_us;
                let tess_us = renderer.tess_us;
                let gpu_name = renderer.gpu_name.clone();
                let demo_texture = renderer.demo_texture;
                let mut settings_open = self.settings_window_open;
                let mut about_open = self.about_modal_open;

                DockArea::new(&mut self.dock_tree, &mut self.dock_state).show(
                    &mut self.ctx,
                    &mut renderer.text,
                    dock_root,
                    |ui, panel| match panel {
                        "Hierarchy" => {
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
                                let camera_resp = ui.tree_node(
                                    selected_entity,
                                    "camera",
                                    "Camera",
                                    |ui, sel| {
                                        ui.selectable_value(sel, "camera_transform", "Transform");
                                        ui.selectable_value(sel, "camera_component", "CameraComponent");
                                        ui.selectable_value(sel, "camera_audio", "AudioListener");
                                    },
                                );
                                ui.context_menu_for(
                                    "ctx_camera",
                                    camera_resp.rect,
                                    |ui| {
                                        let mut _unused = 0u32;
                                        ui.selectable_value(&mut _unused, 1, "Rename");
                                        ui.selectable_value(&mut _unused, 2, "Duplicate");
                                        ui.selectable_value(&mut _unused, 3, "Delete");
                                    },
                                );
                                ui.tree_node(selected_entity, "sun", "Sun", |ui, sel| {
                                    ui.selectable_value(sel, "sun_transform", "Transform");
                                    ui.selectable_value(sel, "sun_light", "DirectionalLight");
                                });
                                ui.tree_node(selected_entity, "world", "World", |ui, sel| {
                                    ui.selectable_value(sel, "world_terrain", "Terrain");
                                    ui.selectable_value(sel, "world_foliage", "Foliage");
                                    ui.selectable_value(sel, "world_water", "Water");
                                    ui.selectable_value(sel, "world_skybox", "Skybox");
                                });
                                ui.tree_node(selected_entity, "props", "Props (12)", |ui, sel| {
                                    for i in 0..12 {
                                        let id_str: &'static str = match i {
                                            0 => "prop_00", 1 => "prop_01", 2 => "prop_02",
                                            3 => "prop_03", 4 => "prop_04", 5 => "prop_05",
                                            6 => "prop_06", 7 => "prop_07", 8 => "prop_08",
                                            9 => "prop_09", 10 => "prop_10", _ => "prop_11",
                                        };
                                        ui.selectable_value(sel, id_str, format!("Prop_{:02}", i));
                                    }
                                });
                                ui.tree_node(selected_entity, "npcs", "NPCs (24)", |ui, sel| {
                                    for i in 0..24 {
                                        // Cheap unique id source: index-based names with a
                                        // common prefix; we just need *something* unique per
                                        // row that's `&'static str`-compatible.
                                        let name = format!("NPC_{:02}", i);
                                        let id_src: &'static str = NPC_IDS[i];
                                        ui.selectable_value(sel, id_src, name);
                                    }
                                });
                                ui.tree_node(selected_entity, "lights", "Lights (8)", |ui, sel| {
                                    for i in 0..8 {
                                        let id_src: &'static str = LIGHT_IDS[i];
                                        ui.selectable_value(sel, id_src, format!("PointLight_{:02}", i));
                                    }
                                });
                            });
                        }
                        "Viewport" => {
                            let st = ui.style();
                            // Phase 13: menu bar with nested submenu. Picks
                            // raise toasts so the action is visible.
                            ui.menu_bar(|ui| {
                                ui.menu_button("File", |ui| {
                                    if ui.menu_item("New Scene") {
                                        ui.toast("Created a new scene");
                                    }
                                    if ui.menu_item("Save") {
                                        ui.toast_success("Scene saved");
                                    }
                                    ui.submenu("Export", |ui| {
                                        if ui.menu_item("glTF") {
                                            ui.toast("Exported glTF");
                                        }
                                        if ui.menu_item("FBX") {
                                            ui.toast("Exported FBX");
                                        }
                                    });
                                    if ui.menu_item("Quit") {
                                        ui.toast_error("Quit is just a demo here");
                                    }
                                });
                                ui.menu_button("Edit", |ui| {
                                    if ui.menu_item("Undo") {
                                        ui.toast("Nothing to undo");
                                    }
                                    let _ = ui.menu_item("Redo");
                                });
                            });
                            ui.add_space(st.spacing.padding);
                            // Phase 13: a draggable asset chip (drag source).
                            let chip_label = "metal.png";
                            let chip = ui.allocate(Vec2::new(120.0, 24.0));
                            let chip_id = ui.alloc_id("asset_metal");
                            let dragging = ui.dnd_drag_source(
                                chip_id,
                                chip,
                                chip_label,
                                || chip_label.to_string(),
                            );
                            let chip_fill = if dragging {
                                st.palette.surface_active
                            } else {
                                st.palette.surface_hover
                            };
                            ui.ctx_mut().paint.push(PaintCmd::Rrect(
                                RectShape::fill(chip, st.rounding.small, chip_fill)
                                    .with_stroke(1.0, st.palette.stroke),
                            ));
                            {
                                let pos = Pos2::new(chip.min.x + 8.0, chip.min.y + 5.0);
                                let mut tmp = Vec::new();
                                ui.text_mut().add_text(
                                    &mut tmp,
                                    chip_label,
                                    st.fonts.small,
                                    pos,
                                    st.palette.text,
                                    None,
                                );
                                ui.ctx_mut().paint.extend(tmp);
                            }
                            ui.add_space(st.spacing.padding);
                            Label::new("Viewport")
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            Label::new("Scene render placeholder")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            // Phase 10: a registered texture sampled through
                            // the new MODE_IMAGE path.
                            Image::new(demo_texture, Vec2::new(160.0, 160.0)).show(ui);
                            ui.add_space(st.spacing.padding);
                            Label::new(format!("Frame: {:.2} ms", frame_time_ms))
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            Label::new(format!("UI pass: {:.0} µs", ui_us))
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            Label::new(format!("Tessellate: {:.0} µs", tess_us))
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            Label::new(format!("Quality: {:?}", *quality))
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.padding);
                            if ui.button("Open Settings window").clicked() {
                                settings_open = true;
                            }
                            if ui.button("Open About (modal)").clicked() {
                                about_open = true;
                            }
                        }
                        "Inspector" => {
                            let st = ui.style();
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
                                Label::new("Theme")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_dim)
                                    .show(ui);
                                ui.horizontal(|ui| {
                                    ui.radio_value(theme, Theme::EditorDark, "Editor");
                                    ui.add_space(ui.style().spacing.padding);
                                    ui.radio_value(theme, Theme::Glassmorphism, "Glass");
                                });
                                ui.add_space(st.spacing.padding);

                                // Phase 13: drop target for the Viewport's
                                // draggable asset chip.
                                Label::new("Material")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_dim)
                                    .show(ui);
                                let slot = ui.allocate(Vec2::new(
                                    ui.available().width(),
                                    28.0,
                                ));
                                let hovering = ui.dnd_hovering::<String>(slot);
                                if let Some(name) = ui.dnd_drop_target::<String>(slot) {
                                    *inspector_slot = Some(name);
                                }
                                let slot_fill = if hovering {
                                    let mut a = st.palette.accent;
                                    a.a = 0.30;
                                    a
                                } else {
                                    st.palette.surface
                                };
                                let slot_stroke = if hovering {
                                    st.palette.accent
                                } else {
                                    st.palette.stroke
                                };
                                ui.ctx_mut().paint.push(PaintCmd::Rrect(
                                    RectShape::fill(slot, st.rounding.small, slot_fill)
                                        .with_stroke(1.0, slot_stroke),
                                ));
                                {
                                    let slot_text = inspector_slot
                                        .as_deref()
                                        .unwrap_or("Drag an asset here");
                                    let pos = Pos2::new(slot.min.x + 8.0, slot.min.y + 7.0);
                                    let mut tmp = Vec::new();
                                    ui.text_mut().add_text(
                                        &mut tmp,
                                        slot_text,
                                        st.fonts.small,
                                        pos,
                                        st.palette.text,
                                        None,
                                    );
                                    ui.ctx_mut().paint.extend(tmp);
                                }
                                ui.add_space(st.spacing.padding);

                                ui.collapsing("Entity", |ui| {
                                    ui.grid("entity_props", |g| {
                                        g.cell(|ui| {
                                            Label::new("Name")
                                                .size(st.fonts.small)
                                                .color(st.palette.text_dim)
                                                .show(ui);
                                        });
                                        g.cell(|ui| {
                                            ui.text_edit(entity_name);
                                        });
                                        g.end_row();
                                        g.cell(|ui| {
                                            Label::new("Source path")
                                                .size(st.fonts.small)
                                                .color(st.palette.text_dim)
                                                .show(ui);
                                        });
                                        g.cell(|ui| {
                                            ui.text_edit(asset_path);
                                        });
                                        g.end_row();
                                    });
                                });
                                ui.add_space(st.spacing.padding);

                                ui.collapsing("Audio", |ui| {
                                    Label::new(format!(
                                        "Volume        {:>3.0} %",
                                        *volume * 100.0
                                    ))
                                    .show(ui);
                                    ui.slider(volume, 0.0..=1.0);
                                });
                                ui.collapsing("Display", |ui| {
                                    Label::new(format!(
                                        "Brightness    {:>3.0} %",
                                        *brightness * 100.0
                                    ))
                                    .show(ui);
                                    ui.slider(brightness, 0.0..=1.0);
                                    ui.add_space(st.spacing.item);
                                    Label::new("Render quality")
                                        .size(st.fonts.small)
                                        .color(st.palette.text_dim)
                                        .show(ui);
                                    let quality_label = match *quality {
                                        Quality::Low => "Low",
                                        Quality::Medium => "Medium",
                                        Quality::High => "High",
                                    };
                                    ComboBox::new("quality")
                                        .selected_text(quality_label)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                quality,
                                                Quality::Low,
                                                "Low",
                                            );
                                            ui.selectable_value(
                                                quality,
                                                Quality::Medium,
                                                "Medium",
                                            );
                                            ui.selectable_value(
                                                quality,
                                                Quality::High,
                                                "High",
                                            );
                                        });
                                });
                                ui.collapsing("Project", |ui| {
                                    ui.checkbox(preview_enabled, "Live preview");
                                    ui.checkbox(auto_save, "Auto-save");
                                });
                                ui.add_space(st.spacing.padding);

                                Label::new("Buffer fill")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_dim)
                                    .show(ui);
                                ui.progress_bar(*volume);
                                ui.add_space(st.spacing.padding);

                                let click_resp = ui.button("Click me");
                                if click_resp.clicked() {
                                    click_delta += 1;
                                }
                                ui.tooltip_for(
                                    click_resp.rect,
                                    "Increments the click counter \u{2014} try right-clicking the Camera in Hierarchy for a context menu",
                                );
                                Label::new(format!("Buttons clicked: {}", click_count))
                                    .size(st.fonts.small)
                                    .color(st.palette.text_dim)
                                    .show(ui);
                            });
                        }
                        "Console" => {
                            let st = ui.style();
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
                                let gpu_line =
                                    format!("[10:23:45] Vulkano 0.35 — {gpu_name}");
                                let lines = [
                                    "[10:23:45] Engine v0.1.0 starting up",
                                    gpu_line.as_str(),
                                    "[10:23:46] Loading project 'demo-scene'...",
                                    "[10:23:46] Compiled 12 shaders",
                                    "[10:23:46] Loaded 218 entities",
                                    "[10:23:47] Texture streaming online (256 MB cache)",
                                    "[10:23:47] Audio engine started",
                                    "[10:23:47] Physics world initialised",
                                    "[10:23:47] Render thread online",
                                    "[10:23:48] Network: bound to 0.0.0.0:7777",
                                    "[10:23:48] Save subsystem ready",
                                    "[10:23:48] Hot-reload watcher: 142 files",
                                    "[10:23:48] Profiler attached on port 8080",
                                    "[10:23:48] Ready.",
                                    "[10:24:01] Camera moved to (12.4, 3.0, -8.2)",
                                    "[10:24:02] Selected entity 'PlayerCamera'",
                                    "[10:24:05] Saved scene",
                                    "[10:24:09] Compiled 1 shader (hot reload)",
                                    "[10:24:14] FPS stabilised at 60",
                                ];
                                for line in lines {
                                    Label::new(line)
                                        .size(st.fonts.small)
                                        .color(st.palette.text_dim)
                                        .show(ui);
                                }
                            });
                        }
                        "Profiler" => {
                            let st = ui.style();
                            Label::new("Frame breakdown (drag a header border to resize, click to sort)")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.padding);

                            // 200 synthetic samples to exercise virtualization.
                            let mut rows: Vec<(usize, &str, f32)> = (0..200)
                                .map(|i| {
                                    let systems = [
                                        "render", "physics", "audio", "scripting", "ui",
                                        "animation", "networking", "streaming",
                                    ];
                                    let name = systems[i % systems.len()];
                                    let ms = 0.05 + ((i * 37 % 100) as f32) * 0.04;
                                    (i, name, ms)
                                })
                                .collect();

                            ui.table(
                                Table::new("profiler")
                                    .row_height(20.0)
                                    .max_height(220.0)
                                    .column(Column::new("#", 48.0).sortable(true))
                                    .column(
                                        Column::new("System", 140.0)
                                            .resizable(true)
                                            .sortable(true),
                                    )
                                    .column(Column::new("ms", 80.0).sortable(true)),
                                |body| {
                                    match body.sort() {
                                        Some((0, dir)) => rows.sort_by(|a, b| {
                                            let o = a.0.cmp(&b.0);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        Some((1, dir)) => rows.sort_by(|a, b| {
                                            let o = a.1.cmp(b.1);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        Some((2, dir)) => rows.sort_by(|a, b| {
                                            let o = a
                                                .2
                                                .partial_cmp(&b.2)
                                                .unwrap_or(std::cmp::Ordering::Equal);
                                            if dir == SortDir::Ascending { o } else { o.reverse() }
                                        }),
                                        _ => {}
                                    }
                                    body.rows(rows.len(), |i, row| {
                                        let (idx, name, ms) = rows[i];
                                        row.cell(|ui| {
                                            ui.label(format!("{idx}"));
                                        });
                                        row.cell(|ui| {
                                            ui.label(name);
                                        });
                                        row.cell(|ui| {
                                            ui.label(format!("{ms:.2}"));
                                        });
                                    });
                                },
                            );
                        }
                        "Errors" => {
                            ui.label("No errors.");
                        }
                        "Warnings" => {
                            let st = ui.style();
                            Label::new("\u{26A0} texture 'metal.png' exceeds 2K target")
                                .size(st.fonts.small)
                                .color(st.palette.accent)
                                .show(ui);
                            Label::new("\u{26A0} 3 unused materials in scene")
                                .size(st.fonts.small)
                                .color(st.palette.accent)
                                .show(ui);
                        }
                        _ => {}
                    },
                );
                self.click_count = self.click_count.saturating_add(click_delta);

                // ── Phase 9: floating windows (own layers, z-ordered,
                // click-to-front; the modal blocks everything beneath).
                let window_slider = &mut self.window_slider;
                self.ctx.run_root(&mut renderer.text, screen_rect, |ui| {
                    GuiWindow::new("Settings")
                        .open(&mut settings_open)
                        .default_pos(Pos2::new(260.0, 140.0))
                        .default_size(Vec2::new(320.0, 260.0))
                        .show(ui, |ui| {
                            ui.label("A floating window:");
                            let st = ui.style();
                            Label::new("drag the title bar, resize at the\nbottom-right grip, collapse with the\nchevron, close with the cross.")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            ui.slider(window_slider, 0.0..=1.0);
                            ui.progress_bar(*window_slider);
                            ui.add_space(st.spacing.item);
                            if ui.button("Open About (modal)").clicked() {
                                about_open = true;
                            }
                        });

                    if about_open {
                        let mut close_clicked = false;
                        GuiWindow::new("About crusty-gui")
                            .open(&mut about_open)
                            .modal(true)
                            .resizable(false)
                            .collapsible(false)
                            .close_on_click_outside(true)
                            .default_pos(Pos2::new(
                                screen_rect.center().x - 170.0,
                                screen_rect.center().y - 110.0,
                            ))
                            .default_size(Vec2::new(340.0, 200.0))
                            .show(ui, |ui| {
                                ui.label("crusty-gui \u{2014} Phase 9");
                                let st = ui.style();
                                Label::new("Modal window: the backdrop is dimmed\nand everything behind is blocked.\nClick outside or press Esc to close.")
                                    .size(st.fonts.small)
                                    .color(st.palette.text_dim)
                                    .show(ui);
                                ui.add_space(st.spacing.item);
                                if ui.button("Close").clicked() {
                                    close_clicked = true;
                                }
                            });
                        if close_clicked {
                            about_open = false;
                        }
                    }

                    // Phase 13: render & reap queued toasts on top of all UI.
                    ui.show_toasts();
                });
                self.settings_window_open = settings_open;
                self.about_modal_open = about_open;

                let output = self.ctx.end_frame();
                let ui_us = ui_start.elapsed().as_secs_f32() * 1e6;
                self.ui_us = self.ui_us * 0.9 + ui_us * 0.1;

                let scene_paint = build_scene_paint(inner_size);
                renderer.render(&scene_paint, &output.paint);
                renderer.window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() {
    let _ = env_logger::try_init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .expect("event loop exited with error");
}
