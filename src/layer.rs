//! Z-ordering for floating regions (Areas, Windows, popups).
//!
//! Every floating region paints into its own layer. At
//! [`Context::end_frame`](crate::context::Context::end_frame) layers are
//! sorted by `(Order, z)` — `Order` is the coarse class below, `z` is a
//! monotonically increasing counter bumped whenever an area is brought to
//! the front — and concatenated after the root paint list.

/// Coarse z-class. Areas in a higher order always render above (and take
/// pointer priority over) areas in a lower one, regardless of `z`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Order {
    /// Root panels and docked UI.
    Background,
    /// Regular floating windows.
    Middle,
    /// Windows pinned above regular ones.
    Foreground,
    /// Modal windows and their dimmed backdrop.
    Modal,
    /// Combo dropdowns, context menus, tooltips. Always on top.
    Tooltip,
}
