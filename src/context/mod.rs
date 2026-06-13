//! Top-level GUI context.
//!
//! [`Context`] owns the [`Memory`] (per-widget state that survives across
//! frames), the per-frame paint list, and the processed pointer/keyboard
//! input state. Widgets read these and append paint commands during the
//! frame.

mod memory;
mod ui;

pub use memory::{AreaState, Memory, WidgetMemory};
pub use ui::{Direction, Ui, UiOptions};

use crate::id::Id;
use crate::input::{Event, Key, Modifiers, MouseButton, RawInput};
use crate::layer::Order;
use crate::math::{Pos2, Rect};
use crate::paint::PaintCmd;
use crate::style::Style;

#[derive(Default)]
pub struct Context {
    /// Per-widget persistent state.
    pub memory: Memory,
    /// Paint commands accumulated this frame.
    pub paint: Vec<PaintCmd>,
    /// Overlay paint commands — appended *after* `paint` at end_frame so
    /// they render on top of the regular UI. Used by floating things:
    /// ComboBox dropdowns, tooltips, context menus.
    pub overlay_paint: Vec<PaintCmd>,
    /// Processed input state (pointer + click flags). Refreshed from
    /// `RawInput.events` at the start of every frame.
    pub input: InputState,
    /// Active style; read by widgets every frame. Swap freely between
    /// frames to retheme the UI.
    pub style: Style,

    screen_rect: Rect,
    pixels_per_point: f32,
    time: f64,
    predicted_dt: f32,
    frame_index: u64,
    inside_frame: bool,

    /// Finished floating-layer command lists for this frame; sorted by
    /// `(Order, z)` and appended after `paint` at end_frame.
    layers: Vec<(Id, Order, Vec<PaintCmd>)>,
    /// Saved parent buffers while painting into a floating layer.
    layer_stack: Vec<Vec<PaintCmd>>,
    /// Area owning the pointer this frame (topmost at the pointer using
    /// last-frame rects; modal-aware). `None` = the root UI.
    hovered_area: Option<Id>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_frame(&mut self, input: RawInput) {
        debug_assert!(
            !self.inside_frame,
            "begin_frame called twice without an intervening end_frame"
        );
        self.screen_rect = input.screen_rect;
        self.pixels_per_point = if input.pixels_per_point > 0.0 {
            input.pixels_per_point
        } else {
            1.0
        };
        self.predicted_dt = input.predicted_dt.max(0.0);
        self.time = input.time;
        self.input.modifiers = input.modifiers;
        self.input.advance_frame();
        for ev in &input.events {
            self.input.apply_event(ev);
        }
        self.paint.clear();
        self.overlay_paint.clear();
        self.layers.clear();
        self.layer_stack.clear();
        self.memory.popup_close_requested = false;
        // Uses last frame's `seen` flags, so this must run before
        // `memory.begin_frame()` clears them.
        self.hovered_area = self.memory.hovered_area(self.input.pointer_pos);
        self.memory.begin_frame();
        // A click anywhere should clear focus; widgets then have a
        // chance to re-claim it during the frame.
        if self.input.pointer_pressed {
            self.memory.focused_widget = None;
        }
        self.inside_frame = true;
    }

    pub fn end_frame(&mut self) -> FrameOutput {
        debug_assert!(self.inside_frame, "end_frame called without begin_frame");
        if self.input.pointer_released {
            self.memory.active_widget = None;
        }
        self.memory.end_frame(8);
        self.inside_frame = false;
        self.frame_index = self.frame_index.wrapping_add(1);
        debug_assert!(
            self.layer_stack.is_empty(),
            "begin_layer without matching end_layer"
        );
        // Floating layers render above the root paint, sorted by
        // (order class, stacking counter); legacy overlay commands
        // (tooltips, combo dropdowns) render last → on very top.
        let mut layers = std::mem::take(&mut self.layers);
        layers.sort_by_key(|(id, order, _)| {
            (*order, self.memory.area(*id).map_or(0, |a| a.z))
        });
        let overlay = std::mem::take(&mut self.overlay_paint);
        let mut paint = std::mem::take(&mut self.paint);
        for (_, _, cmds) in layers {
            paint.extend(cmds);
        }
        paint.extend(overlay);
        FrameOutput {
            screen_rect: self.screen_rect,
            paint,
        }
    }

    /// Redirect subsequent `paint` pushes into a fresh floating layer.
    /// Every call must be paired with [`end_layer`](Self::end_layer).
    pub fn begin_layer(&mut self) {
        self.layer_stack.push(std::mem::take(&mut self.paint));
    }

    /// Close the current floating layer, filing its commands under
    /// `(id, order)` for z-sorted composition at end_frame.
    pub fn end_layer(&mut self, id: Id, order: Order) {
        let parent = self
            .layer_stack
            .pop()
            .expect("end_layer called without begin_layer");
        let cmds = std::mem::replace(&mut self.paint, parent);
        self.layers.push((id, order, cmds));
    }

    /// Area owning the pointer this frame (`None` = root UI).
    #[inline]
    pub fn hovered_area(&self) -> Option<Id> {
        self.hovered_area
    }

    /// True when the pointer is inside `rect` *and* not occluded by (or
    /// blocked from) a floating area other than `area`.
    pub fn pointer_over(&self, area: Option<Id>, rect: Rect) -> bool {
        self.input.pointer_in(rect) && self.hovered_area == area
    }

    /// Run a UI pass within a region. Convenience for the common case of
    /// filling the whole screen with a single top-level vertical layout.
    pub fn run_root<R>(
        &mut self,
        text: &mut crate::text::TextRenderer,
        rect: Rect,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let mut ui = Ui::new(self, text, rect, Direction::TopDown, UiOptions::default());
        f(&mut ui)
    }

    #[inline]
    pub fn screen_rect(&self) -> Rect {
        self.screen_rect
    }
    #[inline]
    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }
    #[inline]
    pub fn dt(&self) -> f32 {
        self.predicted_dt
    }
    #[inline]
    pub fn time(&self) -> f64 {
        self.time
    }
    #[inline]
    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }
}

#[derive(Clone, Debug, Default)]
pub struct FrameOutput {
    pub screen_rect: Rect,
    pub paint: Vec<PaintCmd>,
}

/// Pointer/keyboard state distilled from a frame's [`RawInput::events`].
#[derive(Clone, Debug, Default)]
pub struct InputState {
    pub pointer_pos: Option<Pos2>,
    /// Primary pointer button is currently held.
    pub pointer_down: bool,
    /// Primary pointer button transitioned to "down" this frame.
    pub pointer_pressed: bool,
    /// Primary pointer button transitioned to "up" this frame.
    pub pointer_released: bool,
    /// Secondary pointer button transitioned to "down" this frame
    /// (right-click — used to open context menus).
    pub right_pressed: bool,
    pub scroll_delta: crate::math::Vec2,
    pub modifiers: Modifiers,
    /// Characters typed this frame (composed text, IME-friendly when
    /// winit gives us text). Focused TextEdit appends these to its
    /// value.
    pub typed_text: String,
    /// Keys that transitioned to "pressed" this frame (used by
    /// TextEdit for Backspace / Enter / arrows). Filtered to printable
    /// non-letter keys plus modifiers when needed.
    pub key_presses: Vec<KeyPress>,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

impl InputState {
    fn advance_frame(&mut self) {
        // Edge flags reset every frame; the held flag persists.
        self.pointer_pressed = false;
        self.pointer_released = false;
        self.right_pressed = false;
        self.scroll_delta = crate::math::Vec2::ZERO;
        self.typed_text.clear();
        self.key_presses.clear();
    }

    fn apply_event(&mut self, ev: &Event) {
        match ev {
            Event::PointerMoved(p) => {
                self.pointer_pos = Some(*p);
            }
            Event::PointerGone => {
                self.pointer_pos = None;
            }
            Event::PointerButton {
                pos,
                button,
                pressed,
                ..
            } => {
                self.pointer_pos = Some(*pos);
                if matches!(button, MouseButton::Primary) {
                    if *pressed && !self.pointer_down {
                        self.pointer_pressed = true;
                    }
                    if !*pressed && self.pointer_down {
                        self.pointer_released = true;
                    }
                    self.pointer_down = *pressed;
                }
                if matches!(button, MouseButton::Secondary) && *pressed {
                    self.right_pressed = true;
                }
            }
            Event::Scroll(delta) => {
                self.scroll_delta += *delta;
            }
            Event::Text(s) => {
                self.typed_text.push_str(s);
            }
            Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
            } => {
                if *pressed {
                    self.key_presses.push(KeyPress {
                        key: *key,
                        modifiers: *modifiers,
                        repeat: *repeat,
                    });
                }
            }
            _ => {}
        }
    }

    /// True when the pointer is within `rect` (or there's no pointer).
    pub fn pointer_in(&self, rect: Rect) -> bool {
        self.pointer_pos.map_or(false, |p| rect.contains(p))
    }

    /// Returns true once if a non-repeated press of `key` happened this
    /// frame. Convenience for widgets that don't care about modifiers.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.key_presses.iter().any(|k| k.key == key)
    }
}
