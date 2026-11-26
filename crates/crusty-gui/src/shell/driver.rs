//! The multi-window shell driver: owns the main window, floating dock hosts,
//! the Unreal-style tear-off decorator machinery, and layout persistence.
//! Hand it a [`DockApp`] and run it on a winit event loop — the shell is the
//! `ApplicationHandler`; the app only draws content.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{WindowAttributes, WindowId, WindowLevel};

use crate::context::{Context, CursorIcon, Direction, Ui, UiOptions};
use crate::dock::{DECORATOR_SIZE, DockArea, DockNode, DockResponse, DockState, ExternalDrag};
use crate::id::Id;
use crate::input::{Event, Modifiers, RawInput};
use crate::math::{Pos2, Rect, Vec2};
use crate::window::{GpuContext, WindowRenderer};

use super::app::{DockApp, PanelFrame, ShellCmd, ShellCtl};
use super::drag::{DragTarget, TabDrag, WindowDrag};
use super::host::DockHost;
use super::input::{
    apply_input_event, clear_stuck_pointer, map_modifiers, map_mouse_button, map_scroll,
    push_key_event, window_contains_global,
};
#[cfg(feature = "serde")]
use super::persist;

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

/// Map the library's backend-agnostic [`CursorIcon`] onto winit's.
pub fn winit_cursor(icon: CursorIcon) -> winit::window::CursorIcon {
    use winit::window::CursorIcon as W;
    match icon {
        CursorIcon::Default => W::Default,
        CursorIcon::Pointer => W::Pointer,
        CursorIcon::Text => W::Text,
        CursorIcon::Grab => W::Grab,
        CursorIcon::Grabbing => W::Grabbing,
        CursorIcon::ResizeEw => W::EwResize,
        CursorIcon::ResizeNs => W::NsResize,
        CursorIcon::ResizeNwSe => W::NwseResize,
        CursorIcon::ResizeNeSw => W::NeswResize,
        CursorIcon::Move => W::Move,
        CursorIcon::NotAllowed => W::NotAllowed,
        CursorIcon::Crosshair => W::Crosshair,
    }
}

/// Shell configuration handed to [`Shell::new`] alongside the app.
pub struct ShellConfig {
    /// Main window title.
    pub title: String,
    /// Main window initial logical size.
    pub inner_size: [f32; 2],
    /// Height reserved above the main dock for the app's [`DockApp::top_ui`]
    /// (menu bar / toolbar). Zero skips `top_ui` entirely.
    pub top_bar_height: f32,
    /// Where to persist the dock layout + UI memory. `None` (or building
    /// without the `serde` feature) disables persistence.
    pub layout_path: Option<PathBuf>,
    /// Unreal-style "decorator window" tear-off.
    ///
    /// When `true`, dragging a tab out spawns its real OS window *immediately*
    /// (a borderless, always-on-top "decorator" that follows the cursor); on
    /// drop it either docks into the target and is destroyed, or simply stays
    /// put as a normal float — so a window never pops on top at the end of the
    /// drag.
    ///
    /// When `false`, a windowless ghost is used: the tab is painted as a
    /// translucent card inside whichever window the cursor is over, and a real
    /// window is only spawned on release over empty space.
    pub decorator_windows: bool,
    /// The main window's initial dock tree (replaced by a persisted layout if
    /// one loads).
    pub dock: DockNode,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            title: "crusty-gui".to_string(),
            inner_size: [1280.0, 800.0],
            top_bar_height: 0.0,
            layout_path: None,
            decorator_windows: true,
            dock: DockNode::leaf("Main"),
        }
    }
}

/// The multi-window docking shell. Implements winit's [`ApplicationHandler`]:
/// construct one with [`Shell::new`] and pass it to `EventLoop::run_app`.
pub struct Shell<A: DockApp> {
    app: A,
    title: String,
    inner_size: [f32; 2],
    top_bar_height: f32,
    #[cfg_attr(not(feature = "serde"), allow(dead_code))]
    layout_path: Option<PathBuf>,
    decorator_windows: bool,

    gpu: Option<GpuContext>,
    main_window: Option<WindowRenderer>,
    ctx: Context,
    start: Option<Instant>,
    last_frame_time: f64,
    pointer_pos: Option<Pos2>,
    pending_events: Vec<Event>,
    modifiers: Modifiers,
    dock_tree: DockNode,
    dock_state: DockState,
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
    /// When decorator windows are on, the id of the live decorator window
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
    /// Hash of the last main-window `scene_paint`; a changed scene keeps the
    /// redraw chain alive even when the UI paint is static.
    scene_paint_hash: Option<u64>,
    /// Earliest pending timed repaint (`FrameOutput::repaint_after` — caret
    /// blink, toasts). Serviced in `about_to_wait` via `ControlFlow::WaitUntil`.
    repaint_at: Option<Instant>,
}

impl<A: DockApp> Shell<A> {
    pub fn new(app: A, config: ShellConfig) -> Self {
        let ShellConfig {
            title,
            inner_size,
            top_bar_height,
            layout_path,
            decorator_windows,
            dock,
        } = config;
        Self {
            app,
            title,
            inner_size,
            top_bar_height,
            layout_path,
            decorator_windows,
            gpu: None,
            main_window: None,
            ctx: Context::default(),
            start: None,
            last_frame_time: 0.0,
            pointer_pos: None,
            pending_events: Vec::new(),
            modifiers: Modifiers::empty(),
            dock_tree: dock,
            dock_state: DockState::default(),
            ui_us: 0.0,
            panel_windows: Vec::new(),
            window_drag: None,
            main_incoming: None,
            tab_drag: None,
            main_driving_tearoff: false,
            decorator_id: None,
            decorator_pool: None,
            demoted_host: None,
            drag_settle: 0,
            scene_paint_hash: None,
            repaint_at: None,
        }
    }

    /// Reactive redraw scheduling: keep the frame chain alive only while
    /// something is actually animating (`changed` — this frame's paint
    /// differs, so the *next* frame may too) or an immediate repaint was
    /// requested; park timed requests for `about_to_wait` to service.
    fn schedule_redraw(&mut self, window: &winit::window::Window, changed: bool, repaint_after: Option<Duration>) {
        if changed || repaint_after == Some(Duration::ZERO) {
            window.request_redraw();
        } else if let Some(after) = repaint_after {
            let at = Instant::now() + after;
            self.repaint_at = Some(self.repaint_at.map_or(at, |t| t.min(at)));
        }
    }

    /// Windows' OS modal move/resize loop starves `about_to_wait` and
    /// coalesces redraw requests, so a plain `request_redraw` from
    /// `Moved`/`Resized` renders erratically (~100ms gaps) while the window
    /// is dragged. Render synchronously from the event instead, gated to
    /// ~8ms so event bursts don't outpace the display.
    fn pump_modal_frame(&mut self, event_loop: &ActiveEventLoop) {
        let time = self
            .start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or_default();
        if time - self.last_frame_time >= 0.008 {
            self.render_main(event_loop);
        } else if let Some(main) = self.main_window.as_ref() {
            // Too soon: park a redraw so the final event of the drag still
            // produces an up-to-date frame once the modal loop ends.
            main.window().request_redraw();
        }
    }

    /// The hosted app.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The hosted app, mutably.
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Restore the dock tree + UI memory from `layout_path` if it exists and
    /// parses. A missing or stale file just leaves the defaults in place.
    fn load_layout(&mut self) {
        #[cfg(feature = "serde")]
        if let Some(path) = self.layout_path.as_deref()
            && let Some((dock, memory)) = persist::load_layout(path) {
                self.dock_tree = dock;
                self.ctx.memory = memory;
            }
    }

    /// Write the current dock tree + UI memory to `layout_path`.
    fn save_layout(&self) {
        #[cfg(feature = "serde")]
        if let Some(path) = self.layout_path.as_deref() {
            persist::save_layout(path, &self.dock_tree, &self.ctx.memory);
        }
    }

    /// Apply the [`ShellCmd`]s the app queued during its `top_ui`. Returns
    /// whether a spawned/focused window stole OS focus from the main window
    /// (which then needs one synchronous repaint).
    fn apply_commands(&mut self, event_loop: &ActiveEventLoop, cmds: Vec<ShellCmd>) -> bool {
        let mut focus_stolen = false;
        for cmd in cmds {
            match cmd {
                // Re-open a closed panel by docking it back into the main tree.
                ShellCmd::OpenTab(name) => {
                    self.dock_tree.add_tab(name.as_str());
                }
                // Open a standalone dock window near the main window — unless
                // the tab is already open somewhere, in which case surface it
                // instead of spawning a duplicate.
                ShellCmd::OpenFloating(name) => {
                    if self.dock_tree.contains_tab(&name) {
                        self.dock_tree.activate_tab(&name);
                        if let Some(main) = self.main_window.as_ref() {
                            main.window().request_redraw();
                        }
                    } else if let Some(host) = self
                        .panel_windows
                        .iter()
                        .find(|p| p.tree.contains_tab(&name))
                    {
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
                        self.spawn_dock_host(event_loop, name, g);
                        focus_stolen = true;
                    }
                }
                ShellCmd::SaveLayout => self.save_layout(),
            }
        }
        focus_stolen
    }
}

impl<A: DockApp> ApplicationHandler for Shell<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main_window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title(self.title.clone())
                .with_inner_size(winit::dpi::LogicalSize::new(
                    self.inner_size[0] as f64,
                    self.inner_size[1] as f64,
                ));
            let (gpu, mut window) = WindowRenderer::create_primary(event_loop, attrs);
            self.app.setup_window(&gpu, &mut window);
            self.gpu = Some(gpu);
            self.main_window = Some(window);
            self.start = Some(Instant::now());
            self.load_layout();
            // Pay the decorator window's build cost up front so the first
            // tear-off just shows the warm spare instead of stuttering.
            if self.decorator_windows {
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
            // Not a shell window: it belongs to the app (a window it spawned
            // in `post_frame`), so hand it over.
            self.app
                .app_window_event(event_loop, window_id, event, &mut self.modifiers);
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
                }
                self.pump_modal_frame(event_loop);
            }
            WindowEvent::Moved(_) => self.pump_modal_frame(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Pos2::new(position.x as f32, position.y as f32);
                // Driving a tab tear-off: the main window holds the OS capture,
                // so translate to screen space and pump the windowless tab drag
                // (it follows the cursor as a translucent decorator and lights up
                // drop zones in whatever window it hovers).
                if self.main_driving_tearoff
                    && let Some(main) = self.main_window.as_ref()
                        && let Ok(o) = main.window().inner_position() {
                            let global = Pos2::new(o.x as f32 + pos.x, o.y as f32 + pos.y);
                            if let Some(td) = self.tab_drag.as_mut() {
                                td.last_global = global;
                            }
                            self.route_tab_drag(event_loop, false);
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
        // Redraws are gated on paint changes now, so queued input must wake
        // the frame loop itself.
        if !self.pending_events.is_empty()
            && let Some(main) = self.main_window.as_ref()
        {
            main.window().request_redraw();
        }
    }

    /// Backstop for the rare frame where a drag resolves without an immediate
    /// synchronous `render_main` (e.g. cancel paths): keep the main window and any
    /// overlay-bearing host repainting for a few settle frames so a stale compass
    /// can't linger. The decorator card is never touched — it deliberately freezes
    /// to trail the cursor via DWM compositing.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Service timed repaints (caret blink, toasts) parked by
        // `schedule_redraw`. Due deadlines wake every non-decorator window —
        // unchanged ones re-idle after one cheap hash-skipped frame.
        match self.repaint_at {
            Some(at) if at <= Instant::now() => {
                self.repaint_at = None;
                if let Some(main) = self.main_window.as_ref() {
                    main.window().request_redraw();
                }
                for h in self.panel_windows.iter() {
                    if !h.is_decorator {
                        h.renderer.window().request_redraw();
                    }
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }

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

impl<A: DockApp> Shell<A> {
    /// Render one full main-window frame. Extracted from the `RedrawRequested`
    /// arm so a resolved drop can force an *immediate, synchronous* repaint by
    /// calling this directly. Relying on a deferred `RedrawRequested` fails here:
    /// while a background window holds the drag capture (and the freshly-promoted
    /// titled window steals focus), Windows drops the main window's pending redraw,
    /// leaving the drop-compass frozen on screen until an unrelated event lands.
    fn render_main(&mut self, event_loop: &ActiveEventLoop) {
        // Re-warm the decorator spare on an idle frame (no drag in flight) if a
        // previous promote consumed it, so the next tear-off is hitch-free.
        // No-op when the pool is already full.
        if self.decorator_windows && self.decorator_id.is_none() && self.tab_drag.is_none() {
            self.warm_decorator_spare(event_loop);
        }
        let top_bar_height = self.top_bar_height;
        let Some(renderer) = self.main_window.as_mut() else {
            return;
        };
        renderer.text_mut().new_frame();
        let size = renderer.window().inner_size();
        let pixels_per_point = renderer.window().scale_factor() as f32;
        let time = self
            .start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or_default();
        let dt = if self.last_frame_time > 0.0 {
            // Floor at 1 ms so animations always advance even on
            // back-to-back redraws within the same OS tick.
            (time - self.last_frame_time).clamp(0.001, 0.1) as f32
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
        // Apply the app's style before any widget reads it.
        self.ctx.style = self.app.style();

        let dock_root = Rect::from_min_size(
            Pos2::new(4.0, top_bar_height + 4.0),
            Vec2::new(
                (size.width as f32 - 8.0).max(64.0),
                (size.height as f32 - top_bar_height - 8.0).max(64.0),
            ),
        );

        // Top bar (app-drawn via `DockApp::top_ui`). Collect every open tab
        // first so the app can checkmark a "Window"-style menu.
        let mut cmds: Vec<ShellCmd> = Vec::new();
        if top_bar_height > 0.0 {
            let mut open_tabs = Vec::new();
            self.dock_tree.collect_tabs(&mut open_tabs);
            for p in &self.panel_windows {
                p.tree.collect_tabs(&mut open_tabs);
            }
            let menu_rect =
                Rect::from_min_size(Pos2::ZERO, Vec2::new(size.width as f32, top_bar_height));
            // Small padding so the bar hugs the top — the default 12px from
            // `run_root` would push it down into the dock.
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
            let mut ctl = ShellCtl::new(&open_tabs, &mut cmds);
            self.app.top_ui(&mut ui, &mut ctl);
        }
        let frame = PanelFrame {
            window: renderer.window().id(),
            frame_time_ms: (dt * 1000.0),
            ui_us: self.ui_us,
            tess_us: renderer.tess_us(),
        };
        // Peek (don't take) the incoming drag so the translucent decorator
        // keeps rendering on frames with no cursor movement — routing
        // clears/re-sets it on each move, and we clear it ourselves once a
        // drop is resolved below.
        let redock_ext = self.main_incoming.clone();
        // If a drag was released over the main window this frame, we resolve
        // it (success or cancel) after the dock renders.
        let main_drop_released = redock_ext.as_ref().map(|e| e.released).unwrap_or(false);
        let app = &mut self.app;

        let dock_response = DockArea::new(&mut self.dock_tree, &mut self.dock_state)
            .external_drag(redock_ext)
            .show_close_buttons(true)
            .show(
                &mut self.ctx,
                renderer.text_mut(),
                dock_root,
                |ui, panel| app.panel_ui(ui, panel, &frame),
            );
        let detached = dock_response.detached;
        let docked = dock_response.docked;
        // Demo close policy: accept every close except the last remaining tab.
        if let Some(tab) = dock_response.close_requested {
            let mut all = Vec::new();
            self.dock_tree.collect_tabs(&mut all);
            if all.len() > 1 {
                self.dock_tree.close_tab(&tab);
            }
        }

        // App-owned overlay UI (floating widget windows, modals, toasts).
        self.ctx
            .run_root(renderer.text_mut(), screen_rect, |ui| app.overlay_ui(ui));

        let output = self.ctx.end_frame();
        let ui_us = ui_start.elapsed().as_secs_f32() * 1e6;
        self.ui_us = self.ui_us * 0.9 + ui_us * 0.1;

        renderer.window().set_cursor(winit_cursor(output.cursor_icon));
        let inner_size: [u32; 2] = size.into();
        let scene_paint = self.app.scene_paint(inner_size);
        let scene_hash = crate::paint::paint_hash(&scene_paint);
        let scene_changed = self.scene_paint_hash != Some(scene_hash);
        self.scene_paint_hash = Some(scene_hash);
        renderer.render(&scene_paint, &output.paint);
        let main_win = renderer.window().clone();
        self.schedule_redraw(
            &main_win,
            output.paint_changed || scene_changed,
            output.repaint_after,
        );

        // Opening another window pulls OS focus off the main window, so its
        // own deferred redraw is dropped by Windows and the just-closed menu
        // stays painted until the user clicks back. Track it and paint one
        // more frame synchronously below.
        let mut focus_stolen = false;

        // Let the app spawn or focus its own windows now that the `renderer`
        // borrow has ended and `event_loop` is live.
        if let Some(gpu) = self.gpu.clone() {
            focus_stolen |= self.app.post_frame(&gpu, event_loop);
        }

        // Apply the shell requests the app queued during the frame.
        focus_stolen |= self.apply_commands(event_loop, cmds);

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
                    if self.decorator_windows && self.decorator_id.is_some() {
                        self.promote_decorator(event_loop);
                    } else {
                        self.spawn_dock_host(event_loop, td.tab_id, td.last_global);
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
            // A newly-opened window took focus this frame; repaint the main
            // window now so its closed menu disappears instead of lingering
            // until the user clicks back. The recursive frame has
            // `focus_stolen == false` (no menu click, window already open),
            // so it can't recurse again.
            self.render_main(event_loop);
        }
    }

    /// Spawn a new floating [`DockHost`] window hosting a single-tab dock tree,
    /// positioned so its title strip sits under `at_global` (true screen-space
    /// coordinates). Used both for tearing a tab off a window and for opening a
    /// standalone floating tab. Returns its id so the caller can track it.
    ///
    /// This spawns a normal, titled, opaque float. The translucent tear-off
    /// decorator takes a different path ([`activate_decorator`](Self::activate_decorator)
    /// reusing a warm spare), and a decorator that's dropped over empty space is
    /// upgraded into a normal float *in place*
    /// ([`promote_decorator`](Self::promote_decorator)) rather than spawned.
    fn spawn_dock_host(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab: String,
        at_global: Pos2,
    ) -> Option<WindowId> {
        let gpu = self.gpu.as_ref()?.clone();
        // Position the window in its creation attributes so it appears in place
        // instead of flashing at the OS default spot and jumping (spawn jitter).
        // A real float is grabbed by its title strip at PANEL_GRAB_X.
        let pos = PhysicalPosition::new(
            (at_global.x - PANEL_GRAB_X).round() as i32,
            (at_global.y - PANEL_TITLE_H * 0.5).round() as i32,
        );
        let attrs = WindowAttributes::default()
            .with_title(format!("{} — {tab}", self.title))
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
        let mut renderer = WindowRenderer::new_demotable(&gpu, window);
        self.app.setup_window(&gpu, &mut renderer);
        renderer.window().request_redraw();
        self.panel_windows
            .push(DockHost::new(renderer, DockNode::leaf(tab.as_str())));
        Some(id)
    }

    /// Build a transparent decorator window (Unreal-style cursor card) without
    /// pushing it into `panel_windows`. Used both to warm the spare (hidden) and
    /// as the fallback when a tear-off finds the pool empty. The expensive work
    /// (surface, swapchain, shader pipelines, glyph atlas) happens here, so doing
    /// it ahead of time is what removes the tear-off hitch.
    fn build_decorator_host(
        &mut self,
        event_loop: &ActiveEventLoop,
        pos: PhysicalPosition<i32>,
        visible: bool,
    ) -> Option<DockHost> {
        let gpu = self.gpu.as_ref()?.clone();
        // See `spawn_dock_host` for why the decorator is borderless + transparent
        // + always-on-top + inactive.
        let attrs = WindowAttributes::default()
            .with_title(self.title.clone())
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
        let mut renderer = WindowRenderer::new_transparent(&gpu, window);
        self.app.setup_window(&gpu, &mut renderer);
        let mut host = DockHost::new(renderer, DockNode::leaf("panel"));
        host.is_decorator = true;
        Some(host)
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
        win.set_outer_position(pos);
        win.set_title(&format!("{} — {tab}", self.title));
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
        if let Some(id) = self.decorator_id
            && let Some(h) = self
                .panel_windows
                .iter()
                .find(|h| h.renderer.window().id() == id)
            {
                h.renderer.window().set_outer_position(PhysicalPosition::new(
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
        if self.main_incoming.take().is_some()
            && let Some(main) = self.main_window.as_ref() {
                main.window().request_redraw();
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
        if self.decorator_windows {
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
        if target.is_none()
            && let Some(main) = self.main_window.as_ref()
                && window_contains_global(main.window(), global) {
                    target = Some(DragTarget::Main);
                }

        // Clear last frame's hover everywhere so stale drop zones vanish, and
        // redraw any window we just cleared so it actually repaints without the
        // decorator/compass left over from the previous position (otherwise it
        // stays frozen showing the overlay until the next event reaches it).
        if self.main_incoming.take().is_some()
            && let Some(main) = self.main_window.as_ref() {
                main.window().request_redraw();
            }
        for h in self.panel_windows.iter_mut() {
            if h.incoming.take().is_some() {
                h.renderer.window().request_redraw();
            }
        }

        match target {
            Some(DragTarget::Main) => {
                if let Some(main) = self.main_window.as_ref()
                    && let Ok(o) = main.window().inner_position() {
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
            Some(DragTarget::Host(id)) => {
                if let Some(h) =
                    self.panel_windows.iter_mut().find(|h| h.renderer.window().id() == id)
                    && let Ok(o) = h.renderer.window().inner_position() {
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
            None => {
                // Dropped over no window → float it.
                if released {
                    if self.decorator_windows && self.decorator_id.is_some() {
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
                self.panel_windows[idx].renderer.request_rebuild();
                self.pump_host_modal_frame(event_loop, idx);
            }
            WindowEvent::Moved(_) => self.pump_host_modal_frame(event_loop, idx),
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

    /// Same modal-loop pump as [`pump_modal_frame`](Self::pump_modal_frame),
    /// for a floating dock host. Decorators are exempt — they deliberately
    /// freeze and trail the cursor via DWM compositing.
    fn pump_host_modal_frame(&mut self, event_loop: &ActiveEventLoop, idx: usize) {
        let pw = &self.panel_windows[idx];
        if pw.is_decorator {
            pw.renderer.window().request_redraw();
            return;
        }
        let time = pw.start.elapsed().as_secs_f64();
        if time - pw.last_frame_time < 0.008 {
            pw.renderer.window().request_redraw();
            return;
        }
        let (resp, drop_released) = self.render_dock_host(idx);
        self.resolve_host_frame(event_loop, idx, resp, drop_released);
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
            if self.decorator_windows {
                // Demote: hide the solid source and hand the drag to the warm
                // transparent decorator. `parked` here means "source hidden".
                win.set_visible(false);
                self.panel_windows[idx].parked = true;
                self.decorator_id = self.activate_decorator(event_loop, &rep, global);
            } else {
                win.set_outer_position(PhysicalPosition::new(-30_000, -30_000));
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
        if target.is_none()
            && let Some(main) = self.main_window.as_ref()
                && window_contains_global(main.window(), global) {
                    target = Some(DragTarget::Main);
                }

        // Clear last frame's hover everywhere so stale drop zones vanish.
        self.main_incoming = None;
        for h in self.panel_windows.iter_mut() {
            h.incoming = None;
        }

        match target {
            Some(DragTarget::Main) => {
                if let Some(main) = self.main_window.as_ref()
                    && let Ok(o) = main.window().inner_position() {
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
            Some(DragTarget::Host(id)) => {
                if let Some(h) =
                    self.panel_windows.iter_mut().find(|h| h.renderer.window().id() == id)
                    && let Ok(o) = h.renderer.window().inner_position() {
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
            None => {
                // Released over nothing: cancel and restore the source window.
                if released
                    && let Some(wd) = self.window_drag.take() {
                        self.unpark_source(&wd);
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
                pw.renderer.window().set_outer_position(PhysicalPosition::new(
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
        let app = &mut self.app;
        let pw = &mut self.panel_windows[idx];

        pw.renderer.text_mut().new_frame();
        let size = pw.renderer.window().inner_size();
        let pixels_per_point = pw.renderer.window().scale_factor() as f32;
        let time = pw.start.elapsed().as_secs_f64();
        let dt = if pw.last_frame_time > 0.0 {
            (time - pw.last_frame_time).clamp(0.001, 0.1) as f32
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
        let frame = PanelFrame {
            window: pw.renderer.window().id(),
            frame_time_ms: (dt * 1000.0),
            ui_us,
            tess_us: pw.renderer.tess_us(),
        };
        pw.ctx.begin_frame(input);
        pw.ctx.style = app.style();

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
                |ui, panel| app.panel_ui(ui, panel, &frame),
            );
        let output = pw.ctx.end_frame();
        pw.renderer.window().set_cursor(winit_cursor(output.cursor_icon));
        pw.renderer.render(&[], &output.paint);
        let win = pw.renderer.window().clone();
        self.schedule_redraw(&win, output.paint_changed, output.repaint_after);
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
        let app = &mut self.app;
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
        let frame = PanelFrame {
            window: pw.renderer.window().id(),
            frame_time_ms: 0.0,
            ui_us,
            tess_us: pw.renderer.tess_us(),
        };
        pw.ctx.begin_frame(input);
        pw.ctx.style = app.style();

        let dock_rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(width, height));
        DockArea::new(&mut pw.tree, &mut pw.state).lone_tearoff(true).show(
            &mut pw.ctx,
            pw.renderer.text_mut(),
            dock_rect,
            |ui, panel| app.panel_ui(ui, panel, &frame),
        );
        let mut output = pw.ctx.end_frame();
        // Fade the whole card so the host's drop compass shows through the
        // DWM-composited transparent window.
        crate::paint::scale_alpha(&mut output.paint, 0.72);
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
                if self.decorator_windows {
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
                    if self.decorator_windows && self.decorator_id.is_some() {
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
