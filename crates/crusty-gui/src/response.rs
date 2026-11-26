//! Per-widget interaction result returned by every widget method on
//! [`Ui`](crate::context::Ui).
//!
//! Widgets test the pointer state against the rect they occupied **last
//! frame** (stored in [`Memory`](crate::context::Memory)). This is the
//! standard immediate-mode trick: at the time the widget is *called* we
//! don't know its final layout, so we use the previous frame's rect for
//! hit testing — it's stable enough that the user can't perceive the
//! one-frame lag.

use crate::context::Ui;
use crate::id::Id;
use crate::math::Rect;

#[derive(Clone, Copy, Debug)]
pub struct Response {
    pub id: Id,
    /// Rect this widget occupied this frame.
    pub rect: Rect,
    /// Pointer is currently over the widget's last-frame rect.
    pub hovered: bool,
    /// Primary pointer button is currently held down with the press
    /// having started over this widget.
    pub pressed: bool,
    /// Primary pointer button was released this frame while still
    /// hovering — a successful click.
    pub clicked: bool,
    /// Smoothed [0, 1] hover value, useful for tint/scale animations.
    pub hover_anim: f32,
    /// Smoothed [0, 1] press value, useful for "pushed in" animations.
    pub press_anim: f32,
}

impl Response {
    pub fn clicked(&self) -> bool {
        self.clicked
    }
    /// True when this frame's click completed a double-click (the press
    /// that produced it was quick + near the previous press). Needs the
    /// `Ui` because click timing lives in the shared input state.
    pub fn double_clicked(&self, ui: &Ui) -> bool {
        self.clicked && ui.ctx().input.press_was_double
    }
    pub fn hovered(&self) -> bool {
        self.hovered
    }
    pub fn pressed(&self) -> bool {
        self.pressed
    }
}
