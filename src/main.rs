//! Phase 3 demo binary for crusty-gui.
//!
//! Adds interactive widgets driven by the immediate-mode `Ui` API:
//! pointer events from winit flow through `Context::begin_frame`, the
//! demo closure builds a `Label` and two `Button`s inside the glass
//! panel, and the resulting `PaintCmd` list is tessellated and rendered
//! through the same multi-pass pipeline from Phase 1.5.

use std::sync::Arc;
use std::time::Instant;

use crusty_gui::backend::vulkano::upload_rgba_texture;
use crusty_gui::dock::{DockArea, DockNode, DockState, ExternalDrag};
use crusty_gui::prelude::*;
use crusty_gui::style::Style;
use crusty_gui::widgets::Window as GuiWindow;
use crusty_gui::window::{GpuContext, WindowRenderer};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};

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

/// Every panel `render_panel` knows how to draw. The Window menu lists these
/// so a panel that was closed (or torn off) can always be re-opened.
const ALL_PANELS: [&str; 7] = [
    "Hierarchy", "Viewport", "Inspector", "Console", "Profiler", "Errors", "Warnings",
];

/// Height of the main window's top menu bar strip.
const MENU_BAR_H: f32 = 32.0;

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

/// All the demo's panel-level state, bundled so the same panel content can be
/// rendered from two places: the main window's dock and a popped-out OS
/// window. `render_panel` reads/writes this; the request flags at the bottom
/// are set during a frame and consumed by the caller afterward.
struct EditorState {
    click_count: u32,
    volume: f32,
    brightness: f32,
    preview_enabled: bool,
    auto_save: bool,
    quality: Quality,
    theme: Theme,
    entity_name: String,
    asset_path: String,
    selected_entity: &'static str,
    inspector_slot: Option<String>,
    // Request flags: a panel sets these, the frame loop consumes them.
    settings_open: bool,
    about_open: bool,
    open_mesh_requested: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            click_count: 0,
            volume: 0.6,
            brightness: 0.8,
            preview_enabled: true,
            auto_save: false,
            quality: Quality::Medium,
            theme: Theme::EditorDark,
            entity_name: "PlayerCamera".to_string(),
            asset_path: "assets/scenes/demo.scene".to_string(),
            selected_entity: "",
            inspector_slot: None,
            settings_open: true,
            about_open: false,
            open_mesh_requested: false,
        }
    }
}

/// Per-frame, read-only figures a panel may display. Not persisted.
struct PanelStats {
    frame_time_ms: f32,
    ui_us: f32,
    tess_us: f32,
    gpu_name: String,
    demo_texture: TextureId,
    mesh_already_open: bool,
}

/// Height of the draggable title strip on a torn-off panel window.
const PANEL_TITLE_H: f32 = 26.0;
/// Horizontal cursor offset into the title strip kept under the pointer while
/// a torn-off window follows it.
const PANEL_GRAB_X: f32 = 60.0;

/// A dock panel torn off into its own OS window. Renders the panel's real
/// content via `render_panel`, and carries a manual title-bar drag so the
/// user can fling it back over the main window to re-dock it.
struct PanelWindow {
    renderer: WindowRenderer,
    ctx: Context,
    tab: String,
    demo_texture: TextureId,
    pointer_pos: Option<Pos2>,
    pending_events: Vec<Event>,
    start: Instant,
    last_frame_time: f64,
    /// True while the user holds the title bar to move/re-dock the window.
    dragging: bool,
    /// Cursor offset (window-local) captured when the title drag began.
    drag_grab: Vec2,
    /// True once a re-dock drag has parked this window off-screen, so its solid
    /// body stops covering the main window's drop indicators.
    parked: bool,
    /// Latest true screen-space cursor seen during the drag, used to restore the
    /// window onto the screen if the drop is cancelled (dropped over nothing).
    last_global: Pos2,
}

/// A torn-off panel being dragged back over the main window, expressed in
/// the main window's client-space coordinates so the dock can hit-test it.
struct RedockDrag {
    tab: String,
    pointer: Pos2,
    released: bool,
    /// Re-dock unconditionally on release (window closed) rather than only
    /// over a compass target.
    force: bool,
}

struct App {
    gpu: Option<GpuContext>,
    main_window: Option<WindowRenderer>,
    /// Phase 15: a secondary "Mesh Editor" window, opened on demand, driven
    /// by its own independent [`Context`].
    mesh_window: Option<WindowRenderer>,
    mesh_ctx: Context,
    mesh_pointer_pos: Option<Pos2>,
    mesh_pending_events: Vec<Event>,
    mesh_start: Option<Instant>,
    mesh_last_frame_time: f64,
    mesh_spin: f32,
    demo_texture: Option<TextureId>,
    gpu_name: String,
    ctx: Context,
    start: Option<Instant>,
    last_frame_time: f64,
    pointer_pos: Option<Pos2>,
    pending_events: Vec<Event>,
    modifiers: Modifiers,
    /// All panel-level demo state, shared between the dock and popped-out
    /// panel windows.
    editor: EditorState,
    dock_tree: DockNode,
    dock_state: DockState,
    window_slider: f32,
    ui_us: f32,
    /// Panels torn off into their own OS windows.
    panel_windows: Vec<PanelWindow>,
    /// A panel window currently being dragged back over the main window.
    pending_redock: Option<RedockDrag>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            gpu: None,
            main_window: None,
            mesh_window: None,
            mesh_ctx: Context::default(),
            mesh_pointer_pos: None,
            mesh_pending_events: Vec::new(),
            mesh_start: None,
            mesh_last_frame_time: 0.0,
            mesh_spin: 0.0,
            demo_texture: None,
            gpu_name: String::new(),
            ctx: Context::default(),
            start: None,
            last_frame_time: 0.0,
            pointer_pos: None,
            pending_events: Vec::new(),
            modifiers: Modifiers::empty(),
            editor: EditorState::default(),
            dock_tree: default_dock_tree(),
            dock_state: DockState::default(),
            window_slider: 0.4,
            ui_us: 0.0,
            panel_windows: Vec::new(),
            pending_redock: None,
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
        if self.main_window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title("crusty-gui — Phase 15 (multi-window)")
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
            let (gpu, mut window) = WindowRenderer::create_primary(event_loop, attrs);
            self.gpu_name = gpu.gpu_name.clone();
            self.demo_texture = Some(build_demo_texture(&gpu, &mut window));
            self.gpu = Some(gpu);
            self.main_window = Some(window);
            self.start = Some(Instant::now());
            self.load_layout();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let mesh_id = self.mesh_window.as_ref().map(|w| w.window().id());
        if Some(window_id) == mesh_id {
            self.mesh_event(event_loop, event);
            return;
        }
        if let Some(idx) = self
            .panel_windows
            .iter()
            .position(|p| p.renderer.window().id() == window_id)
        {
            self.panel_event(idx, event);
            return;
        }
        let main_id = self.main_window.as_ref().map(|w| w.window().id());
        if Some(window_id) != main_id {
            return;
        }

        if self.main_window.is_none() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.save_layout();
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                if let Some(renderer) = self.main_window.as_mut() {
                    renderer.request_rebuild();
                    renderer.window().request_redraw();
                }
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
                let pressed = matches!(state, winit::event::ElementState::Pressed);
                let our_button = match button {
                    winit::event::MouseButton::Left => MouseButton::Primary,
                    winit::event::MouseButton::Right => MouseButton::Secondary,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Other(0),
                };
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
                let Some(renderer) = self.main_window.as_mut() else {
                    return;
                };
                renderer.text_mut().new_frame();
                let size = renderer.window().inner_size();
                let pixels_per_point = renderer.window().scale_factor() as f32;
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
                self.ctx.style = self.editor.theme.style();

                let dock_root = Rect::from_min_size(
                    Pos2::new(4.0, MENU_BAR_H + 4.0),
                    Vec2::new(
                        (size.width as f32 - 8.0).max(64.0),
                        (size.height as f32 - MENU_BAR_H - 8.0).max(64.0),
                    ),
                );

                // Top menu bar. The Window menu lists every known panel with a
                // check for the ones currently open (docked or floating); a
                // click re-docks a closed panel so it can never be lost.
                let mut open_flags = [false; ALL_PANELS.len()];
                {
                    let mut existing = Vec::new();
                    self.dock_tree.collect_tabs(&mut existing);
                    for (i, name) in ALL_PANELS.iter().enumerate() {
                        open_flags[i] = existing.iter().any(|t| t == name)
                            || self.panel_windows.iter().any(|p| &p.tab == name);
                    }
                }
                let mut reopen_panel: Option<&'static str> = None;
                let mut save_layout_requested = false;
                let menu_rect = Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(size.width as f32, MENU_BAR_H),
                );
                {
                    // Small padding so the bar hugs the top — the default
                    // 12px from `run_root` would push it down into the dock.
                    let menu_opts = UiOptions {
                        padding: Vec2::new(8.0, 3.0),
                        spacing: 8.0,
                    };
                    let mut ui = Ui::with_root(
                        &mut self.ctx,
                        renderer.text_mut(),
                        menu_rect,
                        Direction::TopDown,
                        menu_opts,
                        Id::ROOT.with("menubar"),
                    );
                    ui.menu_bar(|ui| {
                        ui.menu_button("File", |ui| {
                            if ui.menu_item("Save Layout") {
                                save_layout_requested = true;
                            }
                        });
                        ui.menu_button("Window", |ui| {
                            for (i, name) in ALL_PANELS.iter().enumerate() {
                                let mark = if open_flags[i] { "\u{2713} " } else { "    " };
                                if ui.menu_item(format!("{mark}{name}")) && !open_flags[i] {
                                    reopen_panel = Some(name);
                                }
                            }
                        });
                    });
                }
                let stats = PanelStats {
                    frame_time_ms: (dt * 1000.0) as f32,
                    ui_us: self.ui_us,
                    tess_us: renderer.tess_us(),
                    gpu_name: self.gpu_name.clone(),
                    demo_texture: self.demo_texture.expect("demo texture registered"),
                    mesh_already_open: self.mesh_window.is_some(),
                };
                let redock_ext = self.pending_redock.as_ref().map(|r| ExternalDrag {
                    tab_id: r.tab.clone(),
                    pointer: r.pointer,
                    released: r.released,
                    force: r.force,
                });
                let editor = &mut self.editor;

                let dock_response = DockArea::new(&mut self.dock_tree, &mut self.dock_state)
                    .external_drag(redock_ext)
                    .show(
                        &mut self.ctx,
                        renderer.text_mut(),
                        dock_root,
                        |ui, panel| render_panel(ui, panel, editor, &stats),
                    );
                let detached = dock_response.detached;
                let docked = dock_response.docked;

                // ── Phase 9: floating windows (own layers, z-ordered,
                // click-to-front; the modal blocks everything beneath).
                let window_slider = &mut self.window_slider;
                let editor = &mut self.editor;
                self.ctx.run_root(renderer.text_mut(), screen_rect, |ui| {
                    GuiWindow::new("Settings")
                        .open(&mut editor.settings_open)
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
                                editor.about_open = true;
                            }
                        });

                    if editor.about_open {
                        let mut close_clicked = false;
                        GuiWindow::new("About crusty-gui")
                            .open(&mut editor.about_open)
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
                            editor.about_open = false;
                        }
                    }

                    // Phase 13: render & reap queued toasts on top of all UI.
                    ui.show_toasts();
                });

                let output = self.ctx.end_frame();
                let ui_us = ui_start.elapsed().as_secs_f32() * 1e6;
                self.ui_us = self.ui_us * 0.9 + ui_us * 0.1;

                let scene_paint = build_scene_paint(inner_size);
                renderer.render(&scene_paint, &output.paint);
                renderer.window().request_redraw();

                // Phase 15: a Viewport button asked to spawn the second
                // window. Do it now that the `renderer` borrow has ended and
                // `event_loop` is live.
                if self.editor.open_mesh_requested && self.mesh_window.is_none() {
                    self.editor.open_mesh_requested = false;
                    self.spawn_mesh_window(event_loop);
                }

                // Window menu: re-open a closed panel by docking it back in.
                if let Some(name) = reopen_panel {
                    self.dock_tree.add_tab(name);
                }
                if save_layout_requested {
                    self.save_layout();
                }

                // A tab was dragged off the window edge: tear it off into its
                // own OS window hosting the real panel content.
                if let Some(d) = detached {
                    self.spawn_panel_window(event_loop, d.tab, d.pointer);
                }

                // Resolve an in-flight re-dock once the panel window's title
                // drag is released: if the dock accepted it, destroy the source
                // window; otherwise un-park the window back onto the screen at
                // the release point so it keeps living as its own float.
                if self.pending_redock.as_ref().map(|r| r.released).unwrap_or(false) {
                    if let Some(r) = self.pending_redock.take() {
                        if docked {
                            self.panel_windows.retain(|p| p.tab != r.tab);
                        } else if let Some(pw) =
                            self.panel_windows.iter_mut().find(|p| p.tab == r.tab)
                        {
                            let pos = pw.last_global - pw.drag_grab;
                            let _ = pw.renderer.window().set_outer_position(
                                PhysicalPosition::new(
                                    pos.x.round() as i32,
                                    pos.y.round() as i32,
                                ),
                            );
                            pw.parked = false;
                            pw.renderer.window().request_redraw();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Open the secondary "Mesh Editor" window, sharing the device/queue from
    /// the primary [`GpuContext`] but with its own swapchain and a fresh
    /// independent [`Context`].
    fn spawn_mesh_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let attrs = WindowAttributes::default()
            .with_title("crusty-gui — Mesh Editor")
            .with_inner_size(winit::dpi::LogicalSize::new(560.0, 440.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create mesh-editor window"),
        );
        let renderer = WindowRenderer::new(gpu, window);
        renderer.window().request_redraw();
        self.mesh_window = Some(renderer);
        self.mesh_ctx = Context::default();
        self.mesh_pointer_pos = None;
        self.mesh_pending_events.clear();
        self.mesh_start = Some(Instant::now());
        self.mesh_last_frame_time = 0.0;
    }

    /// Drive the secondary window: its own input buffer, clock, and frame.
    fn mesh_event(&mut self, _event_loop: &ActiveEventLoop, event: WindowEvent) {
        let Some(renderer) = self.mesh_window.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                // Close just this window; the app keeps running.
                self.mesh_window = None;
            }
            WindowEvent::Resized(_) => {
                renderer.request_rebuild();
                renderer.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                renderer.text_mut().new_frame();
                let size = renderer.window().inner_size();
                let pixels_per_point = renderer.window().scale_factor() as f32;
                let time = self
                    .mesh_start
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or_default();
                let dt = if self.mesh_last_frame_time > 0.0 {
                    (time - self.mesh_last_frame_time).max(0.001).min(0.1) as f32
                } else {
                    1.0 / 60.0
                };
                self.mesh_last_frame_time = time;
                self.mesh_spin += dt;

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
                    events: std::mem::take(&mut self.mesh_pending_events),
                };
                self.mesh_ctx.begin_frame(input);
                self.mesh_ctx.style = self.editor.theme.style();

                let spin = self.mesh_spin;
                self.mesh_ctx.run_root(renderer.text_mut(), screen_rect, |ui| {
                    let st = ui.style();
                    ui.add_space(st.spacing.padding);
                    ui.label("Mesh Editor");
                    Label::new(
                        "A second OS window with its own Context,\nswapchain, and glass pipeline — sharing\nthe same GPU device as the main window.",
                    )
                    .size(st.fonts.small)
                    .color(st.palette.text_dim)
                    .show(ui);
                    ui.add_space(st.spacing.item);
                    // A spinning triangle via the Phase 14 painter proves the
                    // window animates off its own independent clock.
                    let canvas = ui.allocate(Vec2::new(220.0, 220.0));
                    let c = canvas.center();
                    let accent = st.palette.accent;
                    let surf = st.palette.surface;
                    let mut p = ui.painter();
                    p.rect_filled(canvas, 6.0, surf);
                    let r = 78.0;
                    let tau = std::f32::consts::TAU;
                    let (a0, a1, a2) = (spin, spin + tau / 3.0, spin + 2.0 * tau / 3.0);
                    p.triangle(
                        Pos2::new(c.x + r * a0.cos(), c.y + r * a0.sin()),
                        Pos2::new(c.x + r * a1.cos(), c.y + r * a1.sin()),
                        Pos2::new(c.x + r * a2.cos(), c.y + r * a2.sin()),
                        accent,
                    );
                });
                let output = self.mesh_ctx.end_frame();
                renderer.render(&[], &output.paint);
                renderer.window().request_redraw();
            }
            other => apply_input_event(
                other,
                &mut self.mesh_pointer_pos,
                &mut self.mesh_pending_events,
                &mut self.modifiers,
            ),
        }
    }

    /// Tear a panel off into its own OS window near where it left the main
    /// window. The window gets its own [`Context`] and a copy of the demo
    /// texture registered on its own [`TextureRegistry`].
    fn spawn_panel_window(&mut self, event_loop: &ActiveEventLoop, tab: String, at: Pos2) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        // Materialize the OS window at the drop point, with its title strip
        // under where the cursor let go. It's a normal focused window now — the
        // drag is already over, so there's no capture trick to preserve.
        let grab = Vec2::new(PANEL_GRAB_X, PANEL_TITLE_H * 0.5);
        let attrs = WindowAttributes::default()
            .with_title(format!("crusty-gui — {tab}"))
            .with_inner_size(winit::dpi::LogicalSize::new(360.0, 420.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create panel window"),
        );
        if let Some(main) = self.main_window.as_ref() {
            if let Ok(origin) = main.window().inner_position() {
                window.set_outer_position(PhysicalPosition::new(
                    origin.x + (at.x - grab.x) as i32,
                    origin.y + (at.y - grab.y) as i32,
                ));
            }
        }
        let mut renderer = WindowRenderer::new(gpu, window);
        let demo_texture = build_demo_texture(gpu, &mut renderer);
        renderer.window().request_redraw();
        self.panel_windows.push(PanelWindow {
            renderer,
            ctx: Context::default(),
            tab,
            demo_texture,
            pointer_pos: None,
            pending_events: Vec::new(),
            start: Instant::now(),
            last_frame_time: 0.0,
            dragging: false,
            drag_grab: Vec2::ZERO,
            parked: false,
            last_global: Pos2::ZERO,
        });
    }

    /// Drive a torn-off panel window: its own input, clock, title-bar drag,
    /// and a frame rendering the real panel content.
    fn panel_event(&mut self, idx: usize, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Don't lose the panel: re-dock it into the main tree.
                let pw = self.panel_windows.remove(idx);
                self.redock_into_main(pw.tab);
            }
            WindowEvent::Resized(_) => {
                let pw = &mut self.panel_windows[idx];
                pw.renderer.request_rebuild();
                pw.renderer.window().request_redraw();
            }
            WindowEvent::RedrawRequested => self.render_panel_window(idx),
            WindowEvent::CursorMoved { position, .. }
                if self.panel_windows[idx].dragging =>
            {
                self.drag_panel_window(idx, Pos2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::MouseInput { state, button, .. }
                if matches!(state, winit::event::ElementState::Released)
                    && matches!(button, winit::event::MouseButton::Left)
                    && self.panel_windows[idx].dragging =>
            {
                // Title drag released: ask the main window to commit the drop.
                self.panel_windows[idx].dragging = false;
                if let Some(r) = self.pending_redock.as_mut() {
                    r.released = true;
                }
                if let Some(main) = self.main_window.as_ref() {
                    main.window().request_redraw();
                }
            }
            other => {
                let pw = &mut self.panel_windows[idx];
                apply_input_event(
                    other,
                    &mut pw.pointer_pos,
                    &mut pw.pending_events,
                    &mut self.modifiers,
                );
            }
        }
    }

    /// While the title bar is held, park the floating window far off-screen so
    /// its solid body stops covering the main window's drop indicators, then
    /// report the cursor (in the main window's client space) so the dock lights
    /// up the live drop zone. The OS keeps the pointer captured here even while
    /// parked, so moves/release keep flowing in.
    fn drag_panel_window(&mut self, idx: usize, local: Pos2) {
        let win = self.panel_windows[idx].renderer.window().clone();
        let Ok(inner) = win.inner_position() else {
            return;
        };
        // Recover the true screen-space cursor from the (possibly parked)
        // window origin plus the window-local cursor. Done before parking so
        // the first move still reads the on-screen origin.
        let global = Pos2::new(inner.x as f32 + local.x, inner.y as f32 + local.y);
        self.panel_windows[idx].last_global = global;
        if !self.panel_windows[idx].parked {
            let _ = win.set_outer_position(PhysicalPosition::new(-30_000, -30_000));
            self.panel_windows[idx].parked = true;
        }
        if let Some(main) = self.main_window.as_ref() {
            if let Ok(morigin) = main.window().inner_position() {
                let mlocal =
                    Pos2::new(global.x - morigin.x as f32, global.y - morigin.y as f32);
                let tab = self.panel_windows[idx].tab.clone();
                self.pending_redock = Some(RedockDrag {
                    tab,
                    pointer: mlocal,
                    released: false,
                    force: false,
                });
                main.window().request_redraw();
            }
        }
    }

    /// Queue a tab to be re-inserted into the main dock on its next frame
    /// (used when a panel window is closed). Drops near the top-left leaf.
    fn redock_into_main(&mut self, tab: String) {
        self.pending_redock = Some(RedockDrag {
            tab,
            pointer: Pos2::new(8.0, 8.0),
            released: true,
            force: true,
        });
        if let Some(main) = self.main_window.as_ref() {
            main.window().request_redraw();
        }
    }

    /// Render a single torn-off panel window: a draggable title strip plus
    /// the panel's real content via [`render_panel`].
    fn render_panel_window(&mut self, idx: usize) {
        let pw = &mut self.panel_windows[idx];
        pw.renderer.text_mut().new_frame();
        let size = pw.renderer.window().inner_size();
        let pixels_per_point = pw.renderer.window().scale_factor() as f32;
        let time = pw.start.elapsed().as_secs_f64();
        let dt = if pw.last_frame_time > 0.0 {
            (time - pw.last_frame_time).max(0.001).min(0.1) as f32
        } else {
            1.0 / 60.0
        };
        pw.last_frame_time = time;
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
            events: std::mem::take(&mut pw.pending_events),
        };
        let stats = PanelStats {
            frame_time_ms: (dt * 1000.0) as f32,
            ui_us: self.ui_us,
            tess_us: pw.renderer.tess_us(),
            gpu_name: self.gpu_name.clone(),
            demo_texture: pw.demo_texture,
            mesh_already_open: self.mesh_window.is_some(),
        };
        let editor = &mut self.editor;
        pw.ctx.begin_frame(input);
        pw.ctx.style = editor.theme.style();
        let tab = pw.tab.clone();
        let width = size.width as f32;
        let st = pw.ctx.style;

        // Draggable title strip across the top.
        let strip = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, PANEL_TITLE_H));
        pw.ctx.paint.push(PaintCmd::Rrect(RectShape::fill(
            strip,
            st.rounding.small,
            st.palette.surface_active,
        )));
        {
            let pos = Pos2::new(strip.min.x + 8.0, strip.min.y + 5.0);
            let label = format!("\u{2725} {tab}  (drag to re-dock)");
            let mut tmp = Vec::new();
            pw.renderer.text_mut().add_text(
                &mut tmp,
                &label,
                st.fonts.small,
                pos,
                st.palette.text_dim,
                None,
            );
            pw.ctx.paint.extend(tmp);
        }

        // Panel content fills a bounded rect *below* the strip — exactly like
        // a dock leaf's body — so `ui.available()` (and the scroll areas that
        // size themselves from it) get the right height instead of the whole
        // window, which previously clipped most of the panel away.
        let content_rect = Rect::from_min_max(
            Pos2::new(0.0, PANEL_TITLE_H),
            Pos2::new(width, size.height as f32),
        );
        let opts = UiOptions {
            padding: Vec2::splat(st.spacing.padding),
            spacing: st.spacing.item,
        };
        pw.ctx.paint.push(PaintCmd::PushClip(content_rect));
        {
            let root_id = Id::ROOT.with("panel").with(&tab);
            let mut ui = Ui::with_root(
                &mut pw.ctx,
                pw.renderer.text_mut(),
                content_rect,
                Direction::TopDown,
                opts,
                root_id,
            );
            render_panel(&mut ui, &tab, editor, &stats);
        }
        pw.ctx.paint.push(PaintCmd::PopClip);
        let output = pw.ctx.end_frame();
        pw.renderer.render(&[], &output.paint);
        pw.renderer.window().request_redraw();

        // Begin a title-bar drag if the user pressed inside the strip.
        if !pw.dragging && pw.ctx.input.pointer_pressed {
            if let Some(p) = pw.ctx.input.pointer_pos {
                if p.y <= PANEL_TITLE_H {
                    pw.dragging = true;
                    pw.drag_grab = Vec2::new(p.x, p.y);
                }
            }
        }
    }
}

/// Render one dock panel's content. Shared by the main window's dock and
/// popped-out OS windows, so a torn-off panel shows its real UI.
fn render_panel(ui: &mut Ui, panel: &str, ed: &mut EditorState, stats: &PanelStats) {
    let volume = &mut ed.volume;
    let brightness = &mut ed.brightness;
    let preview_enabled = &mut ed.preview_enabled;
    let auto_save = &mut ed.auto_save;
    let quality = &mut ed.quality;
    let theme = &mut ed.theme;
    let entity_name = &mut ed.entity_name;
    let asset_path = &mut ed.asset_path;
    let inspector_slot = &mut ed.inspector_slot;
    let selected_entity = &mut ed.selected_entity;
    let click_count = &mut ed.click_count;
    let settings_open = &mut ed.settings_open;
    let about_open = &mut ed.about_open;
    let open_mesh = &mut ed.open_mesh_requested;
    let frame_time_ms = stats.frame_time_ms;
    let ui_us = stats.ui_us;
    let tess_us = stats.tess_us;
    let gpu_name = &stats.gpu_name;
    let demo_texture = stats.demo_texture;
    let mesh_already_open = stats.mesh_already_open;
    match panel {
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
                            let avail = ui.available().height();
                            ui.scroll_area(avail, |ui| {
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
                            // Phase 14: a custom-drawn canvas via the public
                            // Painter — polyline sparkline, filled markers, a
                            // bezier, and text-at-pos, none of which the
                            // widget set covers.
                            {
                                let canvas =
                                    ui.allocate(Vec2::new(220.0, 90.0));
                                let accent = st.palette.accent;
                                let dim = st.palette.text_dim;
                                let surf = st.palette.surface;
                                let mut p = ui.painter();
                                p.rect_filled(canvas, 4.0, surf);
                                // Sparkline across the canvas width.
                                let n = 48;
                                let pts: Vec<Pos2> = (0..n)
                                    .map(|i| {
                                        let t = i as f32 / (n - 1) as f32;
                                        let x = canvas.min.x + 6.0
                                            + t * (canvas.width() - 12.0);
                                        let phase = t * std::f32::consts::TAU * 2.0;
                                        let y = canvas.center().y
                                            - phase.sin() * 22.0
                                            - (t * 6.0).cos() * 6.0;
                                        Pos2::new(x, y)
                                    })
                                    .collect();
                                p.polyline(&pts, 1.5, accent);
                                if let Some(last) = pts.last() {
                                    p.circle_filled(*last, 3.5, accent);
                                }
                                // A bezier swoosh under the line.
                                p.bezier_cubic(
                                    Pos2::new(canvas.min.x + 6.0, canvas.max.y - 8.0),
                                    Pos2::new(canvas.center().x - 30.0, canvas.max.y - 30.0),
                                    Pos2::new(canvas.center().x + 30.0, canvas.max.y + 4.0),
                                    Pos2::new(canvas.max.x - 6.0, canvas.max.y - 14.0),
                                    1.0,
                                    dim,
                                );
                                // A small play-triangle gizmo, top-left.
                                let g = canvas.min;
                                p.triangle(
                                    Pos2::new(g.x + 8.0, g.y + 8.0),
                                    Pos2::new(g.x + 8.0, g.y + 22.0),
                                    Pos2::new(g.x + 20.0, g.y + 15.0),
                                    accent,
                                );
                                p.text(
                                    Pos2::new(canvas.min.x + 26.0, canvas.min.y + 8.0),
                                    "painter()",
                                    12.0,
                                    dim,
                                    None,
                                );
                            }
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
                                *settings_open = true;
                            }
                            if ui.button("Open About (modal)").clicked() {
                                *about_open = true;
                            }
                            // Phase 15: spawn an independent second window.
                            let mesh_label = if mesh_already_open {
                                "Mesh Editor window (open)"
                            } else {
                                "Open Mesh Editor window"
                            };
                            if ui.button(mesh_label).clicked() && !mesh_already_open {
                                *open_mesh = true;
                            }
                            });
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
                                    *click_count += 1;
                                }
                                ui.tooltip_for(
                                    click_resp.rect,
                                    "Increments the click counter \u{2014} try right-clicking the Camera in Hierarchy for a context menu",
                                );
                                Label::new(format!("Buttons clicked: {}", *click_count))
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
    }
}

/// Translate a winit input event into our [`Event`] stream for a window.
/// Non-input events (close/resize/redraw) are handled by the caller and
/// fall through here as no-ops.
fn apply_input_event(
    event: WindowEvent,
    pointer_pos: &mut Option<Pos2>,
    pending_events: &mut Vec<Event>,
    modifiers: &mut Modifiers,
) {
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            let pos = Pos2::new(position.x as f32, position.y as f32);
            *pointer_pos = Some(pos);
            pending_events.push(Event::PointerMoved(pos));
        }
        WindowEvent::CursorLeft { .. } => {
            *pointer_pos = None;
            pending_events.push(Event::PointerGone);
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let our_button = match button {
                winit::event::MouseButton::Left => MouseButton::Primary,
                winit::event::MouseButton::Right => MouseButton::Secondary,
                winit::event::MouseButton::Middle => MouseButton::Middle,
                _ => MouseButton::Other(0),
            };
            let pressed = matches!(state, winit::event::ElementState::Pressed);
            if let Some(pos) = *pointer_pos {
                pending_events.push(Event::PointerButton {
                    pos,
                    button: our_button,
                    pressed,
                    modifiers: *modifiers,
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
            *modifiers = m;
        }
        WindowEvent::MouseWheel { delta, .. } => {
            use winit::event::MouseScrollDelta;
            let v = match delta {
                MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                MouseScrollDelta::PixelDelta(p) => {
                    Vec2::new((p.x as f32) / 20.0, (p.y as f32) / 20.0)
                }
            };
            pending_events.push(Event::Scroll(v));
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
                WKey::Character(s) => s.chars().next().map(Key::Char).unwrap_or(Key::Unknown),
                _ => Key::Unknown,
            };
            pending_events.push(Event::Key {
                key,
                pressed,
                repeat: key_event.repeat,
                modifiers: *modifiers,
            });
            if pressed {
                if let Some(text) = &key_event.text {
                    pending_events.push(Event::Text(text.as_str().to_string()));
                }
            }
        }
        _ => {}
    }
}

/// Build the checkerboard demo texture and register it on `window`'s
/// texture registry, returning its [`TextureId`].
fn build_demo_texture(gpu: &GpuContext, window: &mut WindowRenderer) -> TextureId {
    const N: u32 = 64;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = if on { (90, 130, 200) } else { (30, 40, 60) };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let view = upload_rgba_texture(gpu.device.clone(), gpu.queue.clone(), N, N, &pixels);
    window.registry_mut().register(view, TextureFilter::Nearest)
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
