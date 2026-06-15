//! Docking foundation: a tree of split containers and tabbed leaves,
//! laid out into a target rect each frame.
//!
//! Phase 6 ships:
//!   * Tree data structure with `Split` / `Leaf` nodes plus builders.
//!   * [`DockArea::show`] — walks the tree, computes per-leaf rects,
//!     draws panel chrome (background + tab bar), and invokes a user
//!     callback inside each leaf with a `Ui` for the content area.
//!   * Draggable splitter bars between sibling nodes (live-updating the
//!     parent split's ratio while held).
//!   * Click-to-switch tabs inside a multi-tab leaf.
//!
//! What's deferred to a follow-up phase:
//!   * Drag a tab out of its leaf to re-dock elsewhere.
//!   * 5-way drop zones / preview overlays.
//!   * Layout serialisation (`serde`).
//!   * Tear-off into an OS window (lands with Phase 7 multi-window).

mod area;
mod tree;

pub use area::{DockArea, DockResponse, DockState, DetachedTab, ExternalDrag};
pub use tree::{DockNode, Leaf, Split, SplitDirection, TabId};
