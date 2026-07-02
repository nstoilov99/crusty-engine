//! The app-side contract for shell-hosted docking applications.
//!
//! The shell owns windows, docking, tear-off and input plumbing; the app owns
//! *content*: what each panel draws, the main window's menu bar, and any
//! floating overlay UI. Implement [`DockApp`] and hand it to the shell.

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::context::Ui;
use crate::input::Modifiers;
use crate::paint::PaintCmd;
use crate::style::Style;
use crate::window::{GpuContext, WindowRenderer};

/// Per-frame, per-window figures handed to [`DockApp::panel_ui`], so panel
/// content can display them or pick per-window resources (textures are
/// registered per window renderer).
pub struct PanelFrame {
    /// The OS window this panel is being drawn into.
    pub window: WindowId,
    /// Frame delta of that window, in milliseconds (0 for decorator ghosts).
    pub frame_time_ms: f32,
    /// Smoothed UI-build time of the main window, in microseconds.
    pub ui_us: f32,
    /// Last tessellation time of this window's renderer, in microseconds.
    pub tess_us: f32,
}

/// A request the app makes of the shell during a frame (from menu items,
/// buttons, shortcuts). Queued via [`ShellCtl`] and applied after the frame.
pub enum ShellCmd {
    /// Dock `tab` into the main window's tree (re-open a closed panel).
    OpenTab(String),
    /// Open `tab` as a standalone floating dock window — or, if it is already
    /// open anywhere, surface/focus it instead of duplicating it.
    OpenFloating(String),
    /// Persist the current dock layout + UI memory.
    SaveLayout,
}

/// Command sink + dock queries passed to [`DockApp::top_ui`].
pub struct ShellCtl<'a> {
    /// Every tab currently open in any dock window (main or floating).
    open_tabs: &'a [String],
    cmds: &'a mut Vec<ShellCmd>,
}

impl<'a> ShellCtl<'a> {
    pub fn new(open_tabs: &'a [String], cmds: &'a mut Vec<ShellCmd>) -> Self {
        Self { open_tabs, cmds }
    }

    /// Whether `tab` is currently open in any dock window.
    pub fn is_open(&self, tab: &str) -> bool {
        self.open_tabs.iter().any(|t| t == tab)
    }

    /// Re-dock `tab` into the main window.
    pub fn open_tab(&mut self, tab: impl Into<String>) {
        self.cmds.push(ShellCmd::OpenTab(tab.into()));
    }

    /// Open `tab` as (or surface an existing) floating dock window.
    pub fn open_floating(&mut self, tab: impl Into<String>) {
        self.cmds.push(ShellCmd::OpenFloating(tab.into()));
    }

    /// Persist the dock layout + UI memory.
    pub fn save_layout(&mut self) {
        self.cmds.push(ShellCmd::SaveLayout);
    }
}

/// Panel content and app-owned chrome for a shell-hosted docking application.
///
/// The shell calls these hooks from every window it manages: `panel_ui` runs
/// for each visible dock tab (in the main window, floating hosts, and the
/// translucent tear-off decorator alike); `top_ui` and `overlay_ui` run only
/// in the main window.
pub trait DockApp {
    /// Draw the content of dock tab `tab`.
    fn panel_ui(&mut self, ui: &mut Ui, tab: &str, frame: &PanelFrame);

    /// Style applied to every window's context at the start of its frame.
    fn style(&self) -> Style {
        Style::default()
    }

    /// UI above the main window's dock (menu bar / toolbar). The reserved
    /// height is shell configuration; queue shell requests on `ctl`.
    fn top_ui(&mut self, ui: &mut Ui, ctl: &mut ShellCtl) {
        let _ = (ui, ctl);
    }

    /// Free-floating UI drawn over the main window's dock: floating widget
    /// windows, modals, toasts.
    fn overlay_ui(&mut self, ui: &mut Ui) {
        let _ = ui;
    }

    /// Called once for every window renderer the shell creates (main window,
    /// floating hosts, decorator spares) so the app can register textures on
    /// its registry before content draws.
    fn setup_window(&mut self, gpu: &GpuContext, renderer: &mut WindowRenderer) {
        let _ = (gpu, renderer);
    }

    /// Paint commands rendered *under* the main window's UI (an engine scene,
    /// a backdrop). Defaults to nothing.
    fn scene_paint(&mut self, screen: [u32; 2]) -> Vec<PaintCmd> {
        let _ = screen;
        Vec::new()
    }

    /// Window events for windows the shell does *not* own — windows the app
    /// created itself (e.g. in [`post_frame`](Self::post_frame)). `modifiers`
    /// is the shell's shared keyboard-modifier state; pass it to
    /// [`apply_input_event`](crate::shell::input::apply_input_event).
    fn app_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
        modifiers: &mut Modifiers,
    ) {
        let _ = (event_loop, window_id, event, modifiers);
    }

    /// Called after each main-window frame, with the event loop live, so the
    /// app can spawn or focus OS windows of its own. Return `true` if a window
    /// was created or focused (it stole OS focus from the main window, which
    /// the shell then repaints synchronously).
    fn post_frame(&mut self, gpu: &GpuContext, event_loop: &ActiveEventLoop) -> bool {
        let _ = (gpu, event_loop);
        false
    }
}
