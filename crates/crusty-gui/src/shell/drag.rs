//! In-flight cross-window drag state for the shell's tear-off / re-dock system.

use winit::window::WindowId;

use crate::math::{Pos2, Vec2};

/// A floating [`DockHost`](super::host::DockHost) window being dragged by its
/// title strip to re-dock into another window. While in flight the source
/// window is parked off-screen and the cursor is fed to whichever target
/// window sits under it.
pub struct WindowDrag {
    /// The window being dragged (its tabs move to the drop target).
    pub source: WindowId,
    /// Representative tab fed to the target's drop zones.
    pub rep: String,
    /// The source window's other tabs, transferred into the target on a
    /// successful drop.
    pub extra: Vec<String>,
    /// Cursor offset (window-local) captured when the title drag began.
    pub grab: Vec2,
    /// Latest true screen-space cursor, used to un-park on a cancelled drop.
    pub last_global: Pos2,
}

/// Where a [`WindowDrag`] is currently hovering — the window whose dock will
/// receive the drop.
#[derive(Clone, Copy, PartialEq)]
pub enum DragTarget {
    Main,
    Host(WindowId),
}

/// A single tab torn off a dock and being dragged *without* yet having an OS
/// window of its own (Unreal-style). It renders as a translucent decorator
/// inside whatever window the cursor is over, lights up that window's drop
/// zones, and only materialises into a floating host when dropped over
/// nothing. The window where the press happened holds the OS pointer capture
/// and drives the move/release.
pub struct TabDrag {
    pub tab_id: String,
    /// Cursor offset for the decorator card, so it sits under the pointer the
    /// same way a freshly-spawned floating window's title strip would.
    pub grab: Vec2,
    /// Latest true screen-space cursor.
    pub last_global: Pos2,
}
