//! A floating OS window that is itself a full dock container.

use std::time::Instant;

use crate::context::Context;
use crate::dock::{DockNode, DockState, ExternalDrag};
use crate::input::Event;
use crate::math::{Pos2, Vec2};
use crate::window::WindowRenderer;

/// A floating OS window that is itself a full dock container: it hosts its own
/// [`DockNode`] tree rendered through the same `DockArea` as the main window,
/// so it can hold multiple tabs, be split, accept tabs dropped in from other
/// windows, and have its own tabs torn back out. A thin title strip at the top
/// drags the *whole* window to re-dock it into another window.
pub struct DockHost {
    pub renderer: WindowRenderer,
    pub ctx: Context,
    /// This window's dock layout. Starts as a single-tab leaf when torn off.
    pub tree: DockNode,
    pub state: DockState,
    pub pointer_pos: Option<Pos2>,
    pub pending_events: Vec<Event>,
    pub start: Instant,
    pub last_frame_time: f64,
    /// True while the user holds the title strip to move/re-dock the window.
    pub dragging: bool,
    /// Cursor offset (window-local) captured when the title drag began.
    pub drag_grab: Vec2,
    /// True once a re-dock drag has parked this window off-screen, so its solid
    /// body stops covering the target window's drop indicators.
    pub parked: bool,
    /// Latest true screen-space cursor seen during the drag, used to restore the
    /// window onto the screen if the drop is cancelled (dropped over nothing).
    pub last_global: Pos2,
    /// A tab being dragged in from *another* window this frame; passed straight
    /// to this host's `DockArea` so its drop zones light up.
    pub incoming: Option<ExternalDrag>,
    /// True while *this* window holds the OS pointer capture for a tab tear-off
    /// it just spawned (the press happened here, so its CursorMoved/release must
    /// drive the resulting whole-window drag even though the dragged window is a
    /// different, parked host).
    pub driving_tearoff: bool,
    /// True while this host is the live "decorator window" of an in-flight
    /// tear-off (it follows the cursor and is skipped as a drop target). Cleared
    /// when the drag resolves: either the window is destroyed (docked elsewhere)
    /// or promoted to a normal float in place.
    pub is_decorator: bool,
    /// Frames the decorator has painted since activation. The ghost content is
    /// static while it trails the cursor, so we only repaint the first few
    /// frames (to clear the blank pre-swapchain frame) and then let DWM composite
    /// the moved window without a per-move repaint — that repaint was the
    /// tear-off lag.
    pub decorator_frames: u32,
}

impl DockHost {
    /// A fresh host wrapping `renderer` with the given dock `tree`.
    pub fn new(renderer: WindowRenderer, tree: DockNode) -> Self {
        Self {
            renderer,
            ctx: Context::default(),
            tree,
            state: DockState::default(),
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
        }
    }

    /// A representative tab id for the whole window (used as the dragged tab
    /// when the title strip re-docks this window into another).
    pub fn rep_tab(&self) -> Option<String> {
        let mut tabs = Vec::new();
        self.tree.collect_tabs(&mut tabs);
        tabs.first().cloned()
    }

    /// Every tab id this window currently hosts, in traversal order.
    pub fn all_tabs(&self) -> Vec<String> {
        let mut tabs = Vec::new();
        self.tree.collect_tabs(&mut tabs);
        tabs
    }
}
