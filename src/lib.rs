//! crusty-gui — a hybrid immediate/retained GUI library for the Crusty engine.
//!
//! Phase 0 — only foundational scaffolding is in place. Public surface will
//! grow over subsequent phases.

pub mod animation;
pub mod clipboard;
pub mod math;
pub mod id;
pub mod input;
pub mod context;
pub mod dock;
pub mod layer;
pub mod paint;
pub mod response;
pub mod style;
pub mod text;
pub mod widgets;

#[cfg(feature = "winit-window")]
pub mod window;

#[cfg(feature = "vulkano-renderer")]
pub mod backend;

pub mod prelude {
    pub use crate::context::{Align, Context, Direction, Ui, UiOptions};
    pub use crate::id::Id;
    pub use crate::input::{Event, Key, Modifiers, MouseButton, RawInput, Shortcut};
    pub use crate::layer::Order;
    pub use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
    pub use crate::paint::{Painter, PaintCmd, RectShape, TextureFilter, TextureId};
    pub use crate::response::Response;
    pub use crate::dock::{DockArea, DockNode, SplitDirection};
    pub use crate::style::Style;
    pub use crate::widgets::{
        Area, Button, Checkbox, CollapsingHeader, Column, ComboBox, Grid, GridUi, Image,
        ImageButton, Label, ProgressBar, Radio, ScrollArea, SelectableValue, Slider, SortDir,
        Table, TableBody, TableOutput, TableRow, TextEdit, ToastKind, TreeNode, Window,
    };
}
