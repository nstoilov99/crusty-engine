//! Geometry and color primitives used throughout the crate.
//!
//! These are intentionally small and `Copy`; the public API will take and
//! return them by value almost everywhere.

mod color;
mod rect;
mod rounding;
mod vec2;

pub use color::Color;
pub use rect::Rect;
pub use rounding::Rounding;
pub use vec2::{Pos2, Vec2};
