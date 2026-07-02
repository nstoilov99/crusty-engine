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
use crusty_gui::dock::{DECORATOR_SIZE, DockArea, DockNode, DockResponse, DockState, ExternalDrag};
use crusty_gui::prelude::*;
use crusty_gui::shell::input::{
    apply_input_event, clear_stuck_pointer, map_modifiers, map_mouse_button, map_scroll,
    push_key_event, window_contains_global,
};
use crusty_gui::style::Style;
use crusty_gui::widgets::Window as GuiWindow;
use crusty_gui::window::{GpuContext, WindowRenderer};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId, WindowLevel};

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
const ALL_PANELS: [&str; 10] = [
    "Hierarchy", "Viewport", "Inspector", "Console", "Profiler", "Errors", "Warnings",
    "Texture", "Mesh", "Animation",
];

/// Asset-editor panels — these open as their own standalone dock windows from
/// the Assets menu, the way Unreal opens a texture/mesh/anim editor. They can
/// then be docked into the main window or any other window like any tab.
const ASSET_EDITORS: [&str; 3] = ["Texture", "Mesh", "Animation"];

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

/// Gap between the cursor and the decorator ghost's top-left corner.
///
/// The ghost is a real, opaque, always-on-top window. If it sits *under* the
/// cursor it intercepts the mouse messages the source window's implicit capture
/// needs to drive the drag — so the drop is never seen (ghost stuck on top, tab
/// half-detached). Making it click-through (`WS_EX_TRANSPARENT`) fixes that, but
/// drags in `WS_EX_LAYERED`, which our Vulkan swapchain can't present to (blank
/// white window). Offsetting the ghost down-right of the cursor keeps it out
/// from under the pointer entirely: messages at the cursor hit the window
/// beneath, capture survives, and the ghost stays opaque so it renders normally.
const DECORATOR_CURSOR_GAP: f32 = 14.0;

/// How many frames the decorator repaints after activation before it freezes its
/// content and only repositions. A few frames clear the blank pre-swapchain
/// frame and let any first-frame swapchain recreation settle; after that the
/// trailing ghost is static, so DWM composites the moved window without a repaint.
const DECORATOR_SETTLE_FRAMES: u32 = 4;

/// How many extra frames to force-repaint the main window after a drag resolves,
/// so the cleared drop-compass is flushed even though the just-promoted titled
/// window has stolen focus and the background main window won't repaint itself.
const DRAG_SETTLE_FRAMES: u32 = 4;

/// PROTOTYPE TOGGLE — Unreal-style "decorator window" tear-off.
///
/// When `true`, dragging a tab out spawns its real OS window *immediately* (a
/// borderless, always-on-top "decorator" that follows the cursor); on drop it
/// either docks into the target and is destroyed, or simply stays put as a
/// normal float — so a window never pops on top at the end of the drag.
///
/// When `false`, the original windowless ghost is used: the tab is painted as a
/// translucent card inside whichever window the cursor is over, and a real
/// window is only spawned on release over empty space.
///
/// Flip this to compare the two interaction models side-by-side. (The decorator
/// window is currently opaque — see-through transparency is a follow-up.)
const DECORATOR_WINDOW: bool = true;

/// A floating OS window that is itself a full dock container: it hosts its own
/// [`DockNode`] tree rendered through the same [`DockArea`] as the main window,
/// so it can hold multiple tabs, be split, accept tabs dropped in from other
/// windows, and have its own tabs torn back out. A thin title strip at the top
/// drags the *whole* window to re-dock it into another window.
struct DockHost {
    renderer: WindowRenderer,
    ctx: Context,
    /// This window's dock layout. Starts as a single-tab leaf when torn off.
    tree: DockNode,
    state: DockState,
    demo_texture: TextureId,
    pointer_pos: Option<Pos2>,
    pending_events: Vec<Event>,
    start: Instant,
    last_frame_time: f64,
    /// True while the user holds the title strip to move/re-dock the window.
    dragging: bool,
    /// Cursor offset (window-local) captured when the title drag began.
    drag_grab: Vec2,
    /// True once a re-dock drag has parked this window off-screen, so its solid
    /// body stops covering the target window's drop indicators.
    parked: bool,
    /// Latest true screen-space cursor seen during the drag, used to restore the
    /// window onto the screen if the drop is cancelled (dropped over nothing).
    last_global: Pos2,
    /// A tab being dragged in from *another* window this frame; passed straight
    /// to this host's [`DockArea`] so its drop zones light up.
    incoming: Option<ExternalDrag>,
    /// True while *this* window holds the OS pointer capture for a tab tear-off
    /// it just spawned (the press happened here, so its CursorMoved/release must
    /// drive the resulting whole-window drag even though the dragged window is a
    /// different, parked host).
    driving_tearoff: bool,
    /// True while this host is the live "decorator window" of an in-flight
    /// tear-off (it follows the cursor and is skipped as a drop target). Cleared
    /// when the drag resolves: either the window is destroyed (docked elsewhere)
    /// or promoted to a normal float in place.
    is_decorator: bool,
    /// Frames the decorator has painted since activation. The ghost content is
    /// static while it trails the cursor, so we only repaint the first few
    /// frames (to clear the blank pre-swapchain frame) and then let DWM composite
    /// the moved window without a per-move repaint — that repaint was the
    /// tear-off lag.
    decorator_frames: u32,
}

impl DockHost {
    /// A representative tab id for the whole window (used as the dragged tab
    /// when the title strip re-docks this window into another).
    fn rep_tab(&self) -> Option<String> {
        let mut tabs = Vec::new();
        self.tree.collect_tabs(&mut tabs);
        tabs.first().cloned()
    }

    /// Every tab id this window currently hosts, in traversal order.
    fn all_tabs(&self) -> Vec<String> {
        let mut tabs = Vec::new();
        self.tree.collect_tabs(&mut tabs);
        tabs
    }
}

/// A floating [`DockHost`] window being dragged by its title strip to re-dock
/// into another window. While in flight the source window is parked off-screen
/// and the cursor is fed to whichever target window sits under it.
struct WindowDrag {
    /// The window being dragged (its tabs move to the drop target).
    source: WindowId,
    /// Representative tab fed to the target's drop zones.
    rep: String,
    /// The source window's other tabs, transferred into the target on a
    /// successful drop.
    extra: Vec<String>,
    /// Cursor offset (window-local) captured when the title drag began.
    grab: Vec2,
    /// Latest true screen-space cursor, used to un-park on a cancelled drop.
    last_global: Pos2,
}

/// Where a [`WindowDrag`] is currently hovering — the window whose dock will
/// receive the drop.
#[derive(Clone, Copy, PartialEq)]
enum DragTarget {
    Main,
    Host(WindowId),
}

/// A single tab torn off a dock and being dragged *without* yet having an OS
/// window of its own (Unreal-style). It renders as a translucent decorator
/// inside whatever window the cursor is over, lights up that window's drop
/// zones, and only materialises into a floating [`DockHost`] when dropped over
/// nothing. The window where the press happened holds the OS pointer capture
/// and drives the move/release (see `*_driving_tearoff`).
struct TabDrag {
    tab_id: String,
    /// Cursor offset for the decorator card, so it sits under the pointer the
    /// same way a freshly-spawned floating window's title strip would.
    grab: Vec2,
    /// Latest true screen-space cursor.
    last_global: Pos2,
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
    /// Panels torn off into their own OS windows, each a full dock container.
    panel_windows: Vec<DockHost>,
    /// A floating window currently being dragged by its title strip to re-dock.
    window_drag: Option<WindowDrag>,
    /// A tab being dragged in from a floating window over the *main* window this
    /// frame; fed to the main dock's `external_drag`.
    main_incoming: Option<ExternalDrag>,
    /// A windowless tab tear-off in flight (no OS window until it's dropped over
    /// empty space). Mutually exclusive with `window_drag`.
    tab_drag: Option<TabDrag>,
    /// True while the *main* window holds the OS pointer capture for a tab
    /// tear-off (the press was on a main-dock tab), so the main window's
    /// CursorMoved/release drive the [`TabDrag`].
    main_driving_tearoff: bool,
    /// When [`DECORATOR_WINDOW`] is on, the id of the live decorator window
    /// (a `panel_windows` entry with `is_decorator == true`) for the in-flight
    /// tear-off. `None` when no decorator-window drag is active.
    decorator_id: Option<WindowId>,
    /// A pre-built, hidden transparent decorator window kept warm so a tear-off
    /// can *show* it instead of building a fresh window (surface + swapchain +
    /// shader pipelines) on the drag frame — that synchronous build is what made
    /// tear-off stutter. Refilled after a docked drop (the window is reused) or
    /// lazily on an idle main-window frame after a promote consumes it.
    decorator_pool: Option<DockHost>,
    /// When the live decorator (`decorator_id`) is a floating host demoted *in
    /// place* (its single tab dragged to re-dock), this holds that window's id.
    /// It tells the shared decorator cleanup to *close* the window on a successful
    /// dock (its tab moved elsewhere) instead of returning it to the warm pool.
    /// `None` for an ordinary tear-off whose decorator is the warm spare.
    demoted_host: Option<WindowId>,
    /// Frames to keep force-repainting the main window after a drag *resolves*.
    /// A drop promotes the decorator to a titled window which steals focus; the
    /// now-background main window won't service its own redraw, so the cleared
    /// compass lingers as a stale framebuffer. Pumping a few trailing frames
    /// flushes it. Set on resolution, counted down in `about_to_wait`.
    drag_settle: u32,
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
            window_drag: None,
            main_incoming: None,
            tab_drag: None,
            demoted_host: None,
            drag_settle: 0,
            main_driving_tearoff: false,
            decorator_id: None,
            decorator_pool: None,
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
            // Pay the decorator window's build cost up front so the first
            // tear-off just shows the warm spare instead of stuttering.
            if DECORATOR_WINDOW {
                self.warm_decorator_spare(event_loop);
            }
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
            self.panel_event(event_loop, idx, event);
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
                // Driving a tab tear-off: the main window holds the OS capture,
                // so translate to screen space and pump the windowless tab drag
                // (it follows the cursor as a translucent decorator and lights up
                // drop zones in whatever window it hovers).
                if self.main_driving_tearoff {
                    if let Some(main) = self.main_window.as_ref() {
                        if let Ok(o) = main.window().inner_position() {
                            let global =
                                Pos2::new(o.x as f32 + pos.x, o.y as f32 + pos.y);
                            if let Some(td) = self.tab_drag.as_mut() {
                                td.last_global = global;
                            }
                            self.route_tab_drag(event_loop, false);
                        }
                    }
                }
                self.pointer_pos = Some(pos);
                self.pending_events.push(Event::PointerMoved(pos));
            }
            WindowEvent::CursorLeft { .. } => {
                // Keep the pointer alive while driving a tear-off: the captured
                // cursor can leave the window's client area but the drag must
                // keep tracking it.
                if !self.main_driving_tearoff {
                    self.pointer_pos = None;
                    self.pending_events.push(Event::PointerGone);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = matches!(state, winit::event::ElementState::Pressed);
                // Releasing while driving a tear-off finishes the tab drag (dock
                // into the hovered window, or float it over empty space). Handled
                // before the normal path because the routing relies on the
                // captured screen cursor, not the in-window pointer.
                if !pressed
                    && self.main_driving_tearoff
                    && matches!(button, winit::event::MouseButton::Left)
                {
                    self.main_driving_tearoff = false;
                    // The drag consumed this release, so the main UI never saw the
                    // pointer come up — clear its stuck `pointer_down` so the next
                    // click isn't swallowed.
                    clear_stuck_pointer(self.pointer_pos, &mut self.pending_events, self.modifiers);
                    if self.tab_drag.is_some() {
                        self.route_tab_drag(event_loop, true);
                    } else {
                        // No drag to route: the release can't resolve a drop, so
                        // wipe any compass left lit on a window from an earlier move.
                        self.clear_drag_overlays();
                    }
                    self.close_empty_hosts();
                    // Resolve + repaint the main window synchronously now: its own
                    // deferred redraw would be dropped while another window holds
                    // focus, leaving the compass frozen until a stray event lands.
                    self.render_main(event_loop);
                    return;
                }
                if let Some(pos) = self.pointer_pos {
                    self.pending_events.push(Event::PointerButton {
                        pos,
                        button: map_mouse_button(button),
                        pressed,
                        modifiers: self.modifiers,
                    });
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = map_modifiers(&mods);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.pending_events.push(Event::Scroll(map_scroll(delta)));
            }
            WindowEvent::KeyboardInput { event: key_event, .. } => {
                push_key_event(&key_event, self.modifiers, &mut self.pending_events);
            }
            WindowEvent::RedrawRequested => self.render_main(event_loop),
            _ => {}
        }
    }

    /// Backstop for the rare frame where a drag resolves without an immediate
    /// synchronous `render_main` (e.g. cancel paths): keep the main window and any
    /// overlay-bearing host repainting for a few settle frames so a stale compass
    /// can't linger. The decorator card is never touched — it deliberately freezes
    /// to trail the cursor via DWM compositing.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let drag_active = self.tab_drag.is_some()
            || self.window_drag.is_some()
            || self.main_driving_tearoff
            || self.main_incoming.is_some()
            || self
                .panel_windows
                .iter()
                .any(|h| h.incoming.is_some() || h.driving_tearoff || h.dragging);
        if drag_active {
            self.drag_settle = DRAG_SETTLE_FRAMES;
        } else if self.drag_settle > 0 {
            self.drag_settle -= 1;
        } else {
            return;
        }
        if let Some(main) = self.main_window.as_ref() {
            main.window().request_redraw();
        }
        for h in self.panel_windows.iter() {
            if !h.is_decorator && h.incoming.is_some() {
                h.renderer.window().request_redraw();
            }
        }
    }
}

impl App {
    /// Render one full main-window frame. Extracted from the `RedrawRequested`
    /// arm so a resolved drop can force an *immediate, synchronous* repaint by
    /// calling this directly. Relying on a deferred `RedrawRequested` fails here:
    /// while a background window holds the drag capture (and the freshly-promoted
    /// titled window steals focus), Windows drops the main window's pending redraw,
    /// leaving the drop-compass frozen on screen until an unrelated event lands.
    fn render_main(&mut self, event_loop: &ActiveEventLoop) {
                // Re-warm the decorator spare on an idle frame (no drag in
                // flight) if a previous promote consumed it, so the next
                // tear-off is hitch-free. No-op when the pool is already full.
                if DECORATOR_WINDOW
                    && self.decorator_id.is_none()
                    && self.tab_drag.is_none()
                {
                    self.warm_decorator_spare(event_loop);
                }
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
                            || self.panel_windows.iter().any(|p| p.tree.contains_tab(name));
                    }
                }
                let mut reopen_panel: Option<&'static str> = None;
                let mut open_asset: Option<&'static str> = None;
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
                        ui.menu_button("Assets", |ui| {
                            for name in ASSET_EDITORS {
                                if ui.menu_item(format!("Open {name} Editor")) {
                                    open_asset = Some(name);
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
                // Peek (don't take) the incoming drag so the translucent
                // decorator keeps rendering on frames with no cursor movement —
                // routing clears/re-sets it on each move, and we clear it
                // ourselves once a drop is resolved below.
                let redock_ext = self.main_incoming.clone();
                // If a drag was released over the main window this frame, we
                // resolve it (success or cancel) after the dock renders.
                let main_drop_released =
                    redock_ext.as_ref().map(|e| e.released).unwrap_or(false);
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

                // Opening an editor window pulls OS focus off the main window,
                // so its own deferred redraw is dropped by Windows and the
                // just-closed menu stays painted until the user clicks back.
                // Track it and paint one more frame synchronously below.
                let mut focus_stolen = false;

                // Phase 15: a Viewport button asked to spawn the second
                // window. Do it now that the `renderer` borrow has ended and
                // `event_loop` is live.
                if self.editor.open_mesh_requested && self.mesh_window.is_none() {
                    self.editor.open_mesh_requested = false;
                    self.spawn_mesh_window(event_loop);
                    focus_stolen = true;
                }

                // Window menu: re-open a closed panel by docking it back in.
                if let Some(name) = reopen_panel {
                    self.dock_tree.add_tab(name);
                }

                // Assets menu: open an asset editor in its own standalone dock
                // window, centred over the main window. It can then be docked in.
                // If it's already open (in the main dock or any floating host),
                // focus that instead of spawning a duplicate.
                if let Some(name) = open_asset {
                    if self.dock_tree.contains_tab(name) {
                        // Already docked in the main window — surface its tab.
                        self.dock_tree.activate_tab(name);
                        if let Some(main) = self.main_window.as_ref() {
                            main.window().request_redraw();
                        }
                    } else if let Some(host) = self
                        .panel_windows
                        .iter()
                        .find(|p| p.tree.contains_tab(name))
                    {
                        // Already open as a float — raise and focus it.
                        host.renderer.window().focus_window();
                        host.renderer.window().request_redraw();
                        focus_stolen = true;
                    } else {
                        let g = self
                            .main_window
                            .as_ref()
                            .and_then(|m| m.window().inner_position().ok())
                            .map(|o| Pos2::new(o.x as f32 + 260.0, o.y as f32 + 200.0))
                            .unwrap_or(Pos2::new(260.0, 200.0));
                        self.spawn_dock_host(event_loop, name.to_string(), g);
                        focus_stolen = true;
                    }
                }
                if save_layout_requested {
                    self.save_layout();
                }

                // A tab was torn off the main dock mid-drag: float it in its own
                // window *now* and hand the still-held pointer to the whole-window
                // drag machinery, so it follows the cursor and re-docks like
                // dragging the real window (Unreal-style). The main window holds
                // the OS capture (the press was on a main-dock tab), so it drives.
                if let Some(d) = detached {
                    let g = self
                        .main_window
                        .as_ref()
                        .and_then(|m| m.window().inner_position().ok())
                        .map(|o| Pos2::new(o.x as f32 + d.pointer.x, o.y as f32 + d.pointer.y))
                        .unwrap_or(d.pointer);
                    self.begin_tearoff(event_loop, d.tab, g, DragTarget::Main);
                }

                // A drag was released over the main window this frame.
                if main_drop_released {
                    self.main_incoming = None;
                    if let Some(td) = self.tab_drag.take() {
                        // A tab drag: if the main dock didn't accept it (released
                        // off a drop zone, e.g. on the menu bar), float it so it's
                        // never lost; otherwise the tab now lives in the main dock.
                        if !docked {
                            if DECORATOR_WINDOW && self.decorator_id.is_some() {
                                self.promote_decorator(event_loop);
                            } else {
                                self.spawn_dock_host(
                                    event_loop,
                                    td.tab_id,
                                    td.last_global,
                                );
                            }
                        } else {
                            // Docked into the main window: drop the decorator ghost
                            // and focus main so the next click registers immediately
                            // instead of being eaten by the OS focus change.
                            self.destroy_decorator();
                            if let Some(main) = self.main_window.as_ref() {
                                main.window().focus_window();
                            }
                        }
                    } else if let Some(wd) = self.window_drag.take() {
                        // A whole-window title-strip drag: if its dock accepted
                        // the drop, move the source window's other tabs in too and
                        // destroy it; otherwise un-park it so it keeps floating.
                        if docked {
                            for t in &wd.extra {
                                self.dock_tree.add_tab(t.as_str());
                            }
                            self.panel_windows
                                .retain(|p| p.renderer.window().id() != wd.source);
                            self.destroy_decorator();
                        } else {
                            self.unpark_source(&wd);
                        }
                    }
                }
                // The frame just presented still painted the compass — the drag was
                // `Some(released=true)` while it resolved the drop. Now that the drop
                // is resolved and `main_incoming` is cleared, paint one more frame so
                // the compass actually disappears. The recursive call sees
                // `main_incoming == None`, so `main_drop_released` is false and it
                // can't recurse again.
                if main_drop_released {
                    self.render_main(event_loop);
                } else if focus_stolen {
                    // A newly-opened editor window took focus this frame; repaint
                    // the main window now so its closed menu disappears instead of
                    // lingering until the user clicks back. The recursive frame has
                    // `focus_stolen == false` (no menu click, editor already open),
                    // so it can't recurse again.
                    self.render_main(event_loop);
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

    /// Spawn a new floating [`DockHost`] window hosting a single-tab dock tree,
    /// positioned so its title strip sits under `at_global` (true screen-space
    /// coordinates). Used both for tearing a tab off a window and for opening a
    /// standalone asset editor. Returns its id so the caller can track it.
    ///
    /// This spawns a normal, titled, opaque float. The translucent tear-off
    /// decorator takes a different path ([`activate_decorator`] reusing a warm
    /// spare), and a decorator that's dropped over empty space is upgraded into
    /// a normal float *in place* ([`promote_decorator`]) rather than spawned.
    fn spawn_dock_host(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab: String,
        at_global: Pos2,
    ) -> Option<WindowId> {
        let gpu = self.gpu.as_ref()?;
        // Position the window in its creation attributes so it appears in place
        // instead of flashing at the OS default spot and jumping (spawn jitter).
        // A real float is grabbed by its title strip at PANEL_GRAB_X.
        let pos = PhysicalPosition::new(
            (at_global.x - PANEL_GRAB_X).round() as i32,
            (at_global.y - PANEL_TITLE_H * 0.5).round() as i32,
        );
        let attrs = WindowAttributes::default()
            .with_title(format!("crusty-gui — {tab}"))
            .with_inner_size(winit::dpi::LogicalSize::new(DECORATOR_SIZE.x, DECORATOR_SIZE.y))
            .with_position(pos)
            // Transparent-capable so this float can be demoted into a see-through
            // drag card in place (no title bar) when re-docked by its tab. It
            // still renders fully opaque until then.
            .with_transparent(true);
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create dock-host window"),
        );
        window.set_outer_position(pos);
        let id = window.id();
        let mut renderer = WindowRenderer::new_demotable(gpu, window);
        let demo_texture = build_demo_texture(gpu, &mut renderer);
        renderer.window().request_redraw();
        self.panel_windows.push(DockHost {
            renderer,
            ctx: Context::default(),
            tree: DockNode::leaf(tab.as_str()),
            state: DockState::default(),
            demo_texture,
            pointer_pos: None,
            pending_events: Vec::new(),
            start: Instant::now(),
            last_frame_time: 0.0,
            dragging: false,
            drag_grab: Vec2::ZERO,
            parked: false,
            last_global: Pos2::ZERO,
            incoming: None,
            driving_tearoff: false,
            is_decorator: false,
            decorator_frames: 0,
        });
        Some(id)
    }

    /// Build a transparent decorator window (Unreal-style cursor card) without
    /// pushing it into `panel_windows`. Used both to warm the spare (hidden) and
    /// as the fallback when a tear-off finds the pool empty. The expensive work
    /// (surface, swapchain, shader pipelines, glyph atlas) happens here, so doing
    /// it ahead of time is what removes the tear-off hitch.
    fn build_decorator_host(
        &self,
        event_loop: &ActiveEventLoop,
        pos: PhysicalPosition<i32>,
        visible: bool,
    ) -> Option<DockHost> {
        let gpu = self.gpu.as_ref()?;
        // See `spawn_dock_host` for why the decorator is borderless + transparent
        // + always-on-top + inactive.
        let attrs = WindowAttributes::default()
            .with_title("crusty-gui")
            .with_inner_size(winit::dpi::LogicalSize::new(DECORATOR_SIZE.x, DECORATOR_SIZE.y))
            .with_position(pos)
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            .with_visible(visible);
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create decorator window"),
        );
        let mut renderer = WindowRenderer::new_transparent(gpu, window);
        let demo_texture = build_demo_texture(gpu, &mut renderer);
        Some(DockHost {
            renderer,
            ctx: Context::default(),
            tree: DockNode::leaf("panel"),
            state: DockState::default(),
            demo_texture,
            pointer_pos: None,
            pending_events: Vec::new(),
            start: Instant::now(),
            last_frame_time: 0.0,
            dragging: false,
            drag_grab: Vec2::ZERO,
            parked: false,
            last_global: Pos2::ZERO,
            incoming: None,
            driving_tearoff: false,
            is_decorator: true,
            decorator_frames: 0,
        })
    }

    /// Build the warm decorator spare if we don't already have one. Cheap no-op
    /// when the pool is full; otherwise pays the window-build cost *now* (at
    /// startup or on an idle frame) so a future tear-off doesn't have to.
    fn warm_decorator_spare(&mut self, event_loop: &ActiveEventLoop) {
        if self.decorator_pool.is_some() || self.gpu.is_none() {
            return;
        }
        // Park the hidden spare off-screen; it's repositioned when activated.
        let pos = PhysicalPosition::new(-30_000, -30_000);
        self.decorator_pool = self.build_decorator_host(event_loop, pos, false);
    }

    /// Start a decorator drag by *showing* the warm spare (or building one if the
    /// pool was empty) rather than spawning a fresh window on the drag frame.
    /// Configures it for `tab`, positions it at the cursor, and returns its id.
    fn activate_decorator(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab: &str,
        at_global: Pos2,
    ) -> Option<WindowId> {
        let pos = PhysicalPosition::new(
            (at_global.x + DECORATOR_CURSOR_GAP).round() as i32,
            (at_global.y + DECORATOR_CURSOR_GAP).round() as i32,
        );
        let mut host = match self.decorator_pool.take() {
            Some(h) => h,
            None => self.build_decorator_host(event_loop, pos, true)?,
        };
        host.tree = DockNode::leaf(tab);
        host.state = DockState::default();
        host.is_decorator = true;
        host.start = Instant::now();
        host.last_frame_time = 0.0;
        host.pending_events.clear();
        host.incoming = None;
        host.pointer_pos = None;
        host.dragging = false;
        host.parked = false;
        host.driving_tearoff = false;
        host.decorator_frames = 0;
        let win = host.renderer.window().clone();
        let _ = win.set_outer_position(pos);
        win.set_title(&format!("crusty-gui — {tab}"));
        win.set_visible(true);
        win.request_redraw();
        let id = win.id();
        self.panel_windows.push(host);
        Some(id)
    }

    /// Reposition the live decorator window so it trails just down-right of the
    /// screen-space cursor `global` (kept out from under the pointer so it can't
    /// steal capture). No-op unless a decorator drag is active.
    fn move_decorator(&self, global: Pos2, _grab: Vec2) {
        if let Some(id) = self.decorator_id {
            if let Some(h) = self
                .panel_windows
                .iter()
                .find(|h| h.renderer.window().id() == id)
            {
                let _ = h.renderer.window().set_outer_position(PhysicalPosition::new(
                    (global.x + DECORATOR_CURSOR_GAP).round() as i32,
                    (global.y + DECORATOR_CURSOR_GAP).round() as i32,
                ));
                // The ghost content is static while it trails the cursor, so we
                // only repaint the first few frames (to clear the blank pre-
                // swapchain frame); after that DWM composites the moved window
                // from its last presented frame and a per-move repaint would just
                // re-tessellate the whole faded dock tree — the tear-off lag.
                if h.decorator_frames < DECORATOR_SETTLE_FRAMES {
                    h.renderer.window().request_redraw();
                }
            }
        }
    }

    /// Resolve a decorator tear-off that ended over *no* drop target by
    /// *upgrading the decorator window itself* into a normal titled float — no
    /// new window is spawned, so there's no flash or jump.
    ///
    /// winit 0.30 lets us add OS chrome (`set_decorations`) and drop the
    /// always-on-top level (`set_window_level`) at runtime, and our DWM-
    /// transparent window reads fully opaque the moment we draw opaque pixels
    /// (`WindowRenderer::make_opaque`), so the borderless see-through card
    /// becomes an ordinary panel window in place.
    fn promote_decorator(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(id) = self.decorator_id.take() else {
            return;
        };
        // A demoted-in-place host promotes back to the very window it already is.
        self.demoted_host = None;
        let Some(host) = self
            .panel_windows
            .iter_mut()
            .find(|h| h.renderer.window().id() == id)
        else {
            return;
        };
        // An empty husk (its only tab docked elsewhere) has nothing to promote.
        if host.all_tabs().is_empty() {
            self.panel_windows
                .retain(|h| h.renderer.window().id() != id);
            return;
        }
        host.is_decorator = false;
        let win = host.renderer.window().clone();
        win.set_decorations(true);
        win.set_window_level(WindowLevel::Normal);
        host.renderer.make_opaque();
        host.renderer.request_rebuild();
        win.request_redraw();
        // Activate the freshly-titled window now so its first click starts a drag
        // immediately — otherwise Windows spends that click just activating the
        // background window, forcing the user to click a tab twice to tear it off.
        win.focus_window();
        // The drag is over: the decorator is now a real titled window. Drop any
        // drop-compass still lit on the main window / a target host so it doesn't
        // linger (the freshly-titled window steals focus, starving the background
        // main window of the redraw that would otherwise clear it).
        self.clear_drag_overlays();
        self.drag_settle = DRAG_SETTLE_FRAMES;
    }

    /// Retire the live decorator window — the dragged tab was docked into some
    /// other window, so its card is no longer needed. Rather than destroying the
    /// window (and rebuilding one next tear-off), hide it and return it to the
    /// warm pool for reuse. Only the first goes back; any extra is dropped.
    fn destroy_decorator(&mut self) {
        let Some(id) = self.decorator_id.take() else {
            return;
        };
        // A demoted-in-place host is a *real* float, not the warm spare: its tab
        // moved into the drop target, so close the window outright rather than
        // poisoning the pool with it.
        let was_demoted = self.demoted_host.take() == Some(id);
        let Some(pos) = self
            .panel_windows
            .iter()
            .position(|h| h.renderer.window().id() == id)
        else {
            return;
        };
        let host = self.panel_windows.remove(pos);
        if was_demoted {
            return;
        }
        if self.decorator_pool.is_none() {
            host.renderer.window().set_visible(false);
            self.decorator_pool = Some(host);
        }
    }

    /// Drop any floating host left empty by a lone-tab tear-off. Called once the
    /// drag has resolved (the tab now lives in the drop target or a fresh float),
    /// so the hidden husk window can be destroyed.
    fn close_empty_hosts(&mut self) {
        self.panel_windows.retain(|h| !h.all_tabs().is_empty());
    }

    /// Force every window's drop-compass overlay off and repaint it. Called when a
    /// drag fully resolves so no window is left frozen showing a stale compass —
    /// the per-move clearing in `route_tab_drag` only touches the window the cursor
    /// last hovered, which can leave another window (e.g. the source) still lit.
    fn clear_drag_overlays(&mut self) {
        if self.main_incoming.take().is_some() {
            if let Some(main) = self.main_window.as_ref() {
                main.window().request_redraw();
            }
        }
        for h in self.panel_windows.iter_mut() {
            if h.incoming.take().is_some() {
                h.renderer.window().request_redraw();
            }
        }
    }

    /// Begin a windowless tab tear-off (Unreal-style): the tab is already out of
    /// its old tree, so just start a [`TabDrag`] that follows the cursor as a
    /// translucent decorator inside whatever window it's over. No OS window is
    /// created here — that only happens if the tab is dropped over empty space.
    /// `driver` is the window holding the OS pointer capture (where the press
    /// happened); its `*_driving_tearoff` flag is set so its move/release events
    /// pump this drag instead of its own title-strip logic.
    fn begin_tearoff(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab: String,
        at_global: Pos2,
        driver: DragTarget,
    ) {
        let grab = Vec2::new(PANEL_GRAB_X, PANEL_TITLE_H * 0.5);
        self.tab_drag = Some(TabDrag {
            tab_id: tab.clone(),
            grab,
            last_global: at_global,
        });
        // Unreal-style: materialise the real window up front and let it follow
        // the cursor, so dropping it never pops a fresh window on top.
        if DECORATOR_WINDOW {
            self.decorator_id = self.activate_decorator(event_loop, &tab, at_global);
        }
        match driver {
            DragTarget::Main => self.main_driving_tearoff = true,
            DragTarget::Host(id) => {
                if let Some(h) = self
                    .panel_windows
                    .iter_mut()
                    .find(|h| h.renderer.window().id() == id)
                {
                    h.driving_tearoff = true;
                }
            }
        }
        self.route_tab_drag(event_loop, false);
    }

    /// Re-dock a *whole* floating window by its tab: rather than hiding it and
    /// popping a separate decorator card, demote the window itself in place —
    /// drop its title bar, raise it above the others, and render it see-through —
    /// so it *is* the drag card. It drives the [`TabDrag`] from its own captured
    /// pointer (like a tear-off) and on drop either docks into a target (the
    /// window is then closed) or restores to a normal float in place.
    fn demote_host_in_place(
        &mut self,
        event_loop: &ActiveEventLoop,
        idx: usize,
        tab: String,
        at_global: Pos2,
    ) {
        let grab = Vec2::new(PANEL_GRAB_X, PANEL_TITLE_H * 0.5);
        // The tear-off already pulled the lone tab out; put it back so the window
        // keeps rendering its real content as the (faded) drag card.
        self.panel_windows[idx].tree = DockNode::leaf(tab.as_str());
        let host = &mut self.panel_windows[idx];
        host.is_decorator = true;
        host.driving_tearoff = true;
        host.decorator_frames = 0;
        let win = host.renderer.window().clone();
        let id = win.id();
        win.set_decorations(false);
        win.set_window_level(WindowLevel::AlwaysOnTop);
        host.renderer.make_transparent();
        host.renderer.request_rebuild();
        self.decorator_id = Some(id);
        self.demoted_host = Some(id);
        self.tab_drag = Some(TabDrag {
            tab_id: tab,
            grab,
            last_global: at_global,
        });
        self.route_tab_drag(event_loop, false);
    }

    /// Feed the in-flight [`TabDrag`] to whichever window sits under its cursor:
    /// set that window's `external_drag` so the translucent decorator + drop
    /// zones render there. On `released` over a window the target's own frame
    /// docks it; on `released` over nothing the tab finally materialises into a
    /// new floating [`DockHost`] at the drop point.
    fn route_tab_drag(&mut self, event_loop: &ActiveEventLoop, released: bool) {
        let Some(td) = self.tab_drag.as_ref() else { return };
        let tab = td.tab_id.clone();
        let global = td.last_global;
        let grab = td.grab;

        // Decorator-window mode: the real window IS the ghost, so keep it under
        // the cursor and tell targets to draw their compass but not a second
        // painted card.
        self.move_decorator(global, grab);
        let ghost = self.decorator_id.is_none();

        // Pick the target: floating hosts (on top) first, then the main window.
        // Skip empty hosts — a lone-tab host being torn off is hidden and empty
        // but still geometrically present, and must not catch its own drop.
        // Skip the decorator window too: it sits under the cursor but must never
        // catch its own drop.
        let mut target: Option<DragTarget> = None;
        for h in self.panel_windows.iter() {
            if h.all_tabs().is_empty() || h.is_decorator {
                continue;
            }
            if window_contains_global(h.renderer.window(), global) {
                target = Some(DragTarget::Host(h.renderer.window().id()));
                break;
            }
        }
        if target.is_none() {
            if let Some(main) = self.main_window.as_ref() {
                if window_contains_global(main.window(), global) {
                    target = Some(DragTarget::Main);
                }
            }
        }

        // Clear last frame's hover everywhere so stale drop zones vanish, and
        // redraw any window we just cleared so it actually repaints without the
        // decorator/compass left over from the previous position (otherwise it
        // stays frozen showing the overlay until the next event reaches it).
        if self.main_incoming.take().is_some() {
            if let Some(main) = self.main_window.as_ref() {
                main.window().request_redraw();
            }
        }
        for h in self.panel_windows.iter_mut() {
            if h.incoming.take().is_some() {
                h.renderer.window().request_redraw();
            }
        }

        match target {
            Some(DragTarget::Main) => {
                if let Some(main) = self.main_window.as_ref() {
                    if let Ok(o) = main.window().inner_position() {
                        let local = Pos2::new(global.x - o.x as f32, global.y - o.y as f32);
                        self.main_incoming = Some(ExternalDrag {
                            tab_id: tab,
                            pointer: local,
                            grab,
                            released,
                            force: false,
                            ghost,
                        });
                        main.window().request_redraw();
                    }
                }
            }
            Some(DragTarget::Host(id)) => {
                if let Some(h) =
                    self.panel_windows.iter_mut().find(|h| h.renderer.window().id() == id)
                {
                    if let Ok(o) = h.renderer.window().inner_position() {
                        let local = Pos2::new(global.x - o.x as f32, global.y - o.y as f32);
                        h.incoming = Some(ExternalDrag {
                            tab_id: tab,
                            pointer: local,
                            grab,
                            released,
                            force: false,
                            ghost,
                        });
                        h.renderer.window().request_redraw();
                    }
                }
            }
            None => {
                // Dropped over no window → float it.
                if released {
                    if DECORATOR_WINDOW && self.decorator_id.is_some() {
                        // The decorator window is already here under the cursor:
                        // just keep it as a normal float — nothing new spawns.
                        self.tab_drag = None;
                        self.promote_decorator(event_loop);
                    } else if let Some(td) = self.tab_drag.take() {
                        self.spawn_dock_host(event_loop, td.tab_id, td.last_global);
                    }
                }
            }
        }
    }

    /// Drive a floating dock-host window: its own input, clock, whole-window
    /// title-strip drag, and a frame rendering its dock tree.
    fn panel_event(&mut self, event_loop: &ActiveEventLoop, idx: usize, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Closing a floating window discards its panels; do NOT fold the
                // tabs back into the main dock (they belong to this window now).
                let id = self.panel_windows[idx].renderer.window().id();
                if self.decorator_id == Some(id) {
                    self.decorator_id = None;
                }
                if self.demoted_host == Some(id) {
                    self.demoted_host = None;
                }
                self.panel_windows.remove(idx);
            }
            WindowEvent::Resized(_) => {
                let pw = &mut self.panel_windows[idx];
                pw.renderer.request_rebuild();
                pw.renderer.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if self.panel_windows[idx].is_decorator {
                    // The decorator is a transparent cursor card, not a real dock
                    // host: draw only a small tab thumbnail so the transparent
                    // margin around it actually shows (rendering the full dock
                    // tree would fill every pixel and look opaque).
                    self.render_decorator(idx);
                } else {
                    let (resp, drop_released) = self.render_dock_host(idx);
                    self.resolve_host_frame(event_loop, idx, resp, drop_released);
                }
            }
            WindowEvent::CursorMoved { position, .. }
                if self.panel_windows[idx].dragging =>
            {
                self.drag_panel_window(
                    event_loop,
                    idx,
                    Pos2::new(position.x as f32, position.y as f32),
                );
            }
            WindowEvent::CursorMoved { position, .. }
                if self.panel_windows[idx].driving_tearoff =>
            {
                // This host tore a tab off and holds the OS capture; pump the
                // windowless tab drag from here.
                self.drive_tearoff_from_host(
                    event_loop,
                    idx,
                    Pos2::new(position.x as f32, position.y as f32),
                );
            }
            WindowEvent::MouseInput { state, button, .. }
                if matches!(state, winit::event::ElementState::Released)
                    && matches!(button, winit::event::MouseButton::Left)
                    && self.panel_windows[idx].dragging =>
            {
                // Title drag released: route the drop to whichever window the
                // cursor is over (or cancel if over nothing).
                self.panel_windows[idx].dragging = false;
                let pw = &mut self.panel_windows[idx];
                clear_stuck_pointer(pw.pointer_pos, &mut pw.pending_events, self.modifiers);
                if self.window_drag.is_some() {
                    self.route_window_drag(true);
                }
                self.render_main(event_loop);
            }
            WindowEvent::MouseInput { state, button, .. }
                if matches!(state, winit::event::ElementState::Released)
                    && matches!(button, winit::event::MouseButton::Left)
                    && self.panel_windows[idx].driving_tearoff =>
            {
                // Tear-off drag released from the host that owns the capture.
                self.panel_windows[idx].driving_tearoff = false;
                // This host (the tear-off source) consumed the release; if it
                // survives — or is promoted in place into a titled window — its
                // stuck `pointer_down` would swallow the next click.
                let pw = &mut self.panel_windows[idx];
                clear_stuck_pointer(pw.pointer_pos, &mut pw.pending_events, self.modifiers);
                if self.tab_drag.is_some() {
                    self.route_tab_drag(event_loop, true);
                } else {
                    self.clear_drag_overlays();
                }
                self.close_empty_hosts();
                // Force the main window to resolve + repaint *now*; its deferred
                // redraw is dropped while this host / the promoted window holds
                // focus, which is what left the drop-compass stuck on screen.
                self.render_main(event_loop);
            }
            other => {
                let pw = &mut self.panel_windows[idx];
                apply_input_event(
                    other,
                    &mut pw.pointer_pos,
                    &mut pw.pending_events,
                    &mut self.modifiers,
                );
                // Host windows have no self-sustaining redraw chain (unlike the
                // main window), so queued input would otherwise sit in
                // `pending_events` until some unrelated event triggers a frame —
                // which is why the first click after docking/upgrading needed a
                // second click to register. Request a frame now so the click is
                // processed immediately.
                pw.renderer.window().request_redraw();
            }
        }
    }

    /// While the title strip is held, re-dock the whole window: capture its tabs,
    /// hide its solid body so it stops covering target windows' drop indicators,
    /// and route the drag to whichever window sits underneath. With the decorator
    /// prototype on, the hidden window is replaced by the warm transparent
    /// decorator card (no title bar, see-through so the compass shows through) —
    /// the same smooth path as a tab tear-off. The OS keeps the pointer captured
    /// on the source even while hidden, so moves and the release keep flowing in.
    fn drag_panel_window(&mut self, event_loop: &ActiveEventLoop, idx: usize, local: Pos2) {
        let win = self.panel_windows[idx].renderer.window().clone();
        let Ok(inner) = win.inner_position() else {
            return;
        };
        // Recover the true screen-space cursor from the (possibly hidden) window
        // origin plus the window-local cursor. Done before hiding so the first
        // move still reads the on-screen origin.
        let global = Pos2::new(inner.x as f32 + local.x, inner.y as f32 + local.y);
        self.panel_windows[idx].last_global = global;

        // Lazily begin the window-drag on the first move, capturing the tabs to
        // transfer and demoting the window into a decorator (or parking it).
        if self.window_drag.is_none() {
            let host = &self.panel_windows[idx];
            let Some(rep) = host.rep_tab() else { return };
            let extra: Vec<String> =
                host.all_tabs().into_iter().filter(|t| *t != rep).collect();
            self.window_drag = Some(WindowDrag {
                source: win.id(),
                rep: rep.clone(),
                extra,
                grab: host.drag_grab,
                last_global: global,
            });
            if DECORATOR_WINDOW {
                // Demote: hide the solid source and hand the drag to the warm
                // transparent decorator. `parked` here means "source hidden".
                win.set_visible(false);
                self.panel_windows[idx].parked = true;
                self.decorator_id = self.activate_decorator(event_loop, &rep, global);
            } else {
                let _ = win.set_outer_position(PhysicalPosition::new(-30_000, -30_000));
                self.panel_windows[idx].parked = true;
            }
        } else if let Some(wd) = self.window_drag.as_mut() {
            wd.last_global = global;
        }
        if self.decorator_id.is_some() {
            self.move_decorator(global, self.panel_windows[idx].drag_grab);
        }
        self.route_window_drag(false);
    }

    /// Pump a windowless tab tear-off whose OS pointer capture is held by
    /// floating host `idx` (the window the tab was pulled out of). Translates the
    /// host-local cursor to screen space and routes the [`TabDrag`] to whatever
    /// window sits underneath.
    fn drive_tearoff_from_host(
        &mut self,
        event_loop: &ActiveEventLoop,
        idx: usize,
        local: Pos2,
    ) {
        let win = self.panel_windows[idx].renderer.window().clone();
        let Ok(inner) = win.inner_position() else {
            return;
        };
        let global = Pos2::new(inner.x as f32 + local.x, inner.y as f32 + local.y);
        if let Some(td) = self.tab_drag.as_mut() {
            td.last_global = global;
        }
        self.route_tab_drag(event_loop, false);
    }

    /// Find the window under the dragged window's cursor and feed it an
    /// [`ExternalDrag`] so its drop zones light up. On `released` with no target
    /// the drag is cancelled and the source window restored to the screen.
    fn route_window_drag(&mut self, released: bool) {
        let Some(wd) = self.window_drag.as_ref() else { return };
        let source = wd.source;
        let rep = wd.rep.clone();
        let global = wd.last_global;
        let grab = wd.grab;
        // When demoted to a decorator, the card itself is the drag visual, so the
        // target shows only its drop compass (no in-app translucent ghost).
        let ghost = self.decorator_id.is_none();

        // Pick the target: floating hosts (on top) first, then the main window.
        // Skip the source and the live decorator card so they're never a target.
        let mut target: Option<DragTarget> = None;
        for h in self.panel_windows.iter() {
            if h.renderer.window().id() == source || h.is_decorator {
                continue;
            }
            if window_contains_global(h.renderer.window(), global) {
                target = Some(DragTarget::Host(h.renderer.window().id()));
                break;
            }
        }
        if target.is_none() {
            if let Some(main) = self.main_window.as_ref() {
                if window_contains_global(main.window(), global) {
                    target = Some(DragTarget::Main);
                }
            }
        }

        // Clear last frame's hover everywhere so stale drop zones vanish.
        self.main_incoming = None;
        for h in self.panel_windows.iter_mut() {
            h.incoming = None;
        }

        match target {
            Some(DragTarget::Main) => {
                if let Some(main) = self.main_window.as_ref() {
                    if let Ok(o) = main.window().inner_position() {
                        let local = Pos2::new(global.x - o.x as f32, global.y - o.y as f32);
                        self.main_incoming = Some(ExternalDrag {
                            tab_id: rep,
                            pointer: local,
                            grab,
                            released,
                            force: false,
                            ghost,
                        });
                        main.window().request_redraw();
                    }
                }
            }
            Some(DragTarget::Host(id)) => {
                if let Some(h) =
                    self.panel_windows.iter_mut().find(|h| h.renderer.window().id() == id)
                {
                    if let Ok(o) = h.renderer.window().inner_position() {
                        let local = Pos2::new(global.x - o.x as f32, global.y - o.y as f32);
                        h.incoming = Some(ExternalDrag {
                            tab_id: rep,
                            pointer: local,
                            grab,
                            released,
                            force: false,
                            ghost,
                        });
                        h.renderer.window().request_redraw();
                    }
                }
            }
            None => {
                // Released over nothing: cancel and restore the source window.
                if released {
                    if let Some(wd) = self.window_drag.take() {
                        self.unpark_source(&wd);
                    }
                }
            }
        }
    }

    /// Restore a hidden/parked source window after a cancelled window-drag. With
    /// the decorator prototype on the source was only hidden (never moved), so we
    /// just show it again and retire the decorator card; otherwise it was parked
    /// off-screen and is repositioned at the cursor.
    fn unpark_source(&mut self, wd: &WindowDrag) {
        let demoted = self.decorator_id.is_some();
        if let Some(pw) = self
            .panel_windows
            .iter_mut()
            .find(|p| p.renderer.window().id() == wd.source)
        {
            if demoted {
                pw.renderer.window().set_visible(true);
            } else {
                let pos = wd.last_global - wd.grab;
                let _ = pw.renderer.window().set_outer_position(PhysicalPosition::new(
                    pos.x.round() as i32,
                    pos.y.round() as i32,
                ));
            }
            pw.parked = false;
            pw.renderer.window().request_redraw();
        }
        if demoted {
            self.destroy_decorator();
        }
    }

    /// Render one floating dock-host window: a whole-window drag strip plus its
    /// dock tree via [`DockArea`]. Returns the dock response and whether an
    /// incoming cross-window drag was released over it this frame.
    fn render_dock_host(&mut self, idx: usize) -> (DockResponse, bool) {
        // A lone-tab host whose tab is mid-tear-off is empty and hidden; there's
        // nothing to draw and `DockArea` would index an empty leaf. It still
        // drives the drag via its captured pointer, so just skip the frame.
        if self.panel_windows[idx].all_tabs().is_empty() {
            return (DockResponse::default(), false);
        }
        let modifiers = self.modifiers;
        let ui_us = self.ui_us;
        let gpu_name = self.gpu_name.clone();
        let mesh_open = self.mesh_window.is_some();
        let editor = &mut self.editor;
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
        let width = size.width as f32;
        let height = size.height as f32;
        let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
        let input = RawInput {
            screen_rect,
            pixels_per_point,
            time,
            predicted_dt: dt,
            modifiers,
            events: std::mem::take(&mut pw.pending_events),
        };
        let stats = PanelStats {
            frame_time_ms: (dt * 1000.0) as f32,
            ui_us,
            tess_us: pw.renderer.tess_us(),
            gpu_name,
            demo_texture: pw.demo_texture,
            mesh_already_open: mesh_open,
        };
        pw.ctx.begin_frame(input);
        pw.ctx.style = editor.theme.style();

        // The dock tree fills the whole window — no drag strip. The window is
        // moved with the OS title bar; grabbing its tab re-docks it.
        let dock_rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(width, height));
        // Peek (don't take) so the decorator persists on still frames; routing
        // clears/re-sets it per move and `resolve_host_frame` clears it on drop.
        let incoming = pw.incoming.clone();
        let drop_released = incoming.as_ref().map(|e| e.released).unwrap_or(false);
        let resp = DockArea::new(&mut pw.tree, &mut pw.state)
            .external_drag(incoming)
            .lone_tearoff(true)
            .show(
                &mut pw.ctx,
                pw.renderer.text_mut(),
                dock_rect,
                |ui, panel| render_panel(ui, panel, editor, &stats),
            );
        let output = pw.ctx.end_frame();
        pw.renderer.render(&[], &output.paint);
        pw.renderer.window().request_redraw();
        (resp, drop_released)
    }

    /// Paint the live decorator window: the dragged panel's *real* content,
    /// faded as one unit, on an otherwise fully transparent window — Unreal's
    /// translucent drag thumbnail. Because the window is DWM-transparent, the
    /// faded pixels let the drop compass in the window beneath show through, and
    /// the empty margin around the panel reads as pure see-through.
    fn render_decorator(&mut self, idx: usize) {
        let modifiers = self.modifiers;
        let ui_us = self.ui_us;
        let gpu_name = self.gpu_name.clone();
        let mesh_open = self.mesh_window.is_some();
        let editor = &mut self.editor;
        let pw = &mut self.panel_windows[idx];

        pw.renderer.text_mut().new_frame();
        let size = pw.renderer.window().inner_size();
        let pixels_per_point = pw.renderer.window().scale_factor() as f32;
        let time = pw.start.elapsed().as_secs_f64();
        let width = size.width as f32;
        let height = size.height as f32;
        let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
        let input = RawInput {
            screen_rect,
            pixels_per_point,
            time,
            predicted_dt: 1.0 / 60.0,
            modifiers,
            events: std::mem::take(&mut pw.pending_events),
        };
        let stats = PanelStats {
            frame_time_ms: 0.0,
            ui_us,
            tess_us: pw.renderer.tess_us(),
            gpu_name,
            demo_texture: pw.demo_texture,
            mesh_already_open: mesh_open,
        };
        pw.ctx.begin_frame(input);
        pw.ctx.style = editor.theme.style();

        let dock_rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(width, height));
        DockArea::new(&mut pw.tree, &mut pw.state).lone_tearoff(true).show(
            &mut pw.ctx,
            pw.renderer.text_mut(),
            dock_rect,
            |ui, panel| render_panel(ui, panel, editor, &stats),
        );
        let mut output = pw.ctx.end_frame();
        // Fade the whole card so the host's drop compass shows through the
        // DWM-composited transparent window.
        crusty_gui::paint::scale_alpha(&mut output.paint, 0.72);
        pw.renderer.render(&[], &output.paint);
        pw.decorator_frames = pw.decorator_frames.saturating_add(1);
    }

    /// Resolve a dock-host frame: tear-offs spawn new windows; a committed
    /// cross-window drop folds in the source window's other tabs and closes it;
    /// a released-but-missed drop cancels and restores the source.
    fn resolve_host_frame(
        &mut self,
        event_loop: &ActiveEventLoop,
        idx: usize,
        resp: DockResponse,
        drop_released: bool,
    ) {
        // A tab was torn off this host mid-drag: start a windowless tab drag and
        // hand the still-held pointer to it. This window holds the OS capture, so
        // it drives the drag. If that was the host's last tab the window is now
        // empty — hide it so its stale frame doesn't linger; it stays alive only
        // to pump the drag and is closed when the drag resolves.
        if let Some(d) = resp.detached {
            let host_id = self.panel_windows[idx].renderer.window().id();
            let g = self.panel_windows[idx]
                .renderer
                .window()
                .inner_position()
                .ok()
                .map(|o| Pos2::new(o.x as f32 + d.pointer.x, o.y as f32 + d.pointer.y))
                .unwrap_or(d.pointer);
            if self.panel_windows[idx].all_tabs().is_empty() {
                // Lone tab: the *whole* window is being re-docked. Demote it in
                // place (borderless, see-through, follows the cursor) instead of
                // hiding it and popping a separate decorator window.
                if DECORATOR_WINDOW {
                    self.demote_host_in_place(event_loop, idx, d.tab, g);
                    return;
                }
                self.panel_windows[idx].renderer.window().set_visible(false);
            }
            self.begin_tearoff(event_loop, d.tab, g, DragTarget::Host(host_id));
            return;
        }

        // A drag was released over this host this frame.
        if drop_released {
            self.panel_windows[idx].incoming = None;
            if let Some(td) = self.tab_drag.take() {
                // Tab drag missed this host's drop zones → float it.
                if !resp.docked {
                    if DECORATOR_WINDOW && self.decorator_id.is_some() {
                        self.promote_decorator(event_loop);
                    } else {
                        self.spawn_dock_host(event_loop, td.tab_id, td.last_global);
                    }
                } else {
                    // Docked into this host: focus it so the next click registers
                    // immediately, then drop the decorator ghost.
                    self.panel_windows[idx].renderer.window().focus_window();
                    self.destroy_decorator();
                }
            } else if let Some(wd) = self.window_drag.take() {
                // Whole-window title-strip drag dropped onto this host.
                if resp.docked {
                    let host_id = self.panel_windows[idx].renderer.window().id();
                    if let Some(h) = self
                        .panel_windows
                        .iter_mut()
                        .find(|h| h.renderer.window().id() == host_id)
                    {
                        for t in &wd.extra {
                            h.tree.add_tab(t.as_str());
                        }
                    }
                    self.panel_windows
                        .retain(|p| p.renderer.window().id() != wd.source);
                    self.destroy_decorator();
                } else {
                    self.unpark_source(&wd);
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
                        "Texture" => {
                            ui.label("metal_albedo.png");
                            let st = ui.style();
                            Label::new("2048 \u{00D7} 2048 \u{00B7} BC7 \u{00B7} 5.3 MB")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            Image::new(demo_texture, Vec2::new(200.0, 200.0)).show(ui);
                            ui.add_space(st.spacing.item);
                            ui.checkbox(preview_enabled, "sRGB");
                            ui.checkbox(auto_save, "Generate mipmaps");
                        }
                        "Mesh" => {
                            ui.label("character_lod0.mesh");
                            let st = ui.style();
                            Label::new("12,480 tris \u{00B7} 6,210 verts \u{00B7} 1 material")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            Image::new(demo_texture, Vec2::new(220.0, 160.0)).show(ui);
                            ui.add_space(st.spacing.item);
                            ui.label("LOD bias");
                            ui.slider(brightness, 0.0..=1.0);
                            ui.checkbox(preview_enabled, "Wireframe");
                        }
                        "Animation" => {
                            ui.label("run_cycle.anim");
                            let st = ui.style();
                            Label::new("32 frames \u{00B7} 30 fps \u{00B7} 1.07 s \u{00B7} looping")
                                .size(st.fonts.small)
                                .color(st.palette.text_dim)
                                .show(ui);
                            ui.add_space(st.spacing.item);
                            ui.label("Timeline");
                            ui.slider(volume, 0.0..=1.0);
                            ui.progress_bar(*volume);
                            ui.add_space(st.spacing.item);
                            ui.checkbox(preview_enabled, "Play");
                            ui.checkbox(auto_save, "Loop");
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
