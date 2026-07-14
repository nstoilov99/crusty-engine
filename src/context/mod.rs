//! Top-level GUI context.
//!
//! [`Context`] owns the [`Memory`] (per-widget state that survives across
//! frames), the per-frame paint list, and the processed pointer/keyboard
//! input state. Widgets read these and append paint commands during the
//! frame.

mod dnd;
mod memory;
mod ui;

pub use dnd::DragDrop;
pub use memory::{AreaState, Memory, WidgetMemory};
pub use ui::{Align, Direction, Ui, UiOptions};

use std::time::Duration;

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
    /// In-flight drag-and-drop payload (type-erased).
    pub dnd: DragDrop,

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
    /// Explicit pointer claim for this frame (e.g. a dock tab drag whose
    /// state lives outside `Memory`); reset at begin_frame, OR-ed into
    /// [`FrameOutput::owns_pointer`].
    pointer_claimed: bool,
    /// Cursor requested by widgets this frame; reset to `Default` at
    /// begin_frame, reported through [`FrameOutput::cursor_icon`].
    cursor_icon: CursorIcon,
    /// Earliest repaint request made this frame (`None` = only repaint on
    /// new input). Reported through [`FrameOutput::repaint_after`].
    repaint_after: Option<Duration>,
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
        self.input.detect_double_press(input.time);
        self.paint.clear();
        self.overlay_paint.clear();
        self.layers.clear();
        self.layer_stack.clear();
        self.cursor_icon = CursorIcon::Default;
        self.repaint_after = None;
        self.pointer_claimed = false;
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
            // Any drop target had its chance during the frame; a release
            // that reaches here means the drag ended over empty space.
            self.dnd.clear();
        }
        // Tab / Shift-Tab moves keyboard focus among widgets that registered
        // as focusable this frame. Handled here, after the whole UI has been
        // built, so the focus order is complete; the change lands next frame.
        if let Some(tab) = self.input.key_presses.iter().find(|k| k.key == Key::Tab) {
            let backward = tab.modifiers.contains(Modifiers::SHIFT);
            self.memory.focus_next(backward);
        }
        // The engine seam: does the UI claim this frame's pointer/keyboard,
        // or may the host route them to the scene underneath?
        let owns_pointer = self.pointer_claimed
            || self.hovered_area.is_some()
            || self.memory.active_widget.is_some()
            || self.dnd.is_dragging();
        let wants_pointer =
            owns_pointer || self.memory.any_widget_contains(self.input.pointer_pos);
        let wants_keyboard = self.memory.focused_widget.is_some();
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
            wants_pointer,
            owns_pointer,
            wants_keyboard,
            cursor_icon: self.cursor_icon,
            repaint_after: self.repaint_after,
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

    /// Request a cursor for this frame (hovering a text field, a resize
    /// edge, …). Later callers win, so widgets drawn on top override the
    /// ones below them. Resets to [`CursorIcon::Default`] every frame.
    #[inline]
    pub fn set_cursor_icon(&mut self, icon: CursorIcon) {
        self.cursor_icon = icon;
    }

    /// Claim this frame's pointer for the UI (reported through
    /// [`FrameOutput::owns_pointer`]) even when no widget is active or
    /// hovered — e.g. a dock tab drag whose state lives outside `Memory`.
    /// Resets every frame.
    #[inline]
    pub fn claim_pointer(&mut self) {
        self.pointer_claimed = true;
    }

    /// Ask the host to repaint immediately (an animation is running).
    #[inline]
    pub fn request_repaint(&mut self) {
        self.request_repaint_after(Duration::ZERO);
    }

    /// Ask the host to repaint no later than `after` from now (a blinking
    /// caret, a timed toast). Multiple requests keep the earliest.
    pub fn request_repaint_after(&mut self, after: Duration) {
        self.repaint_after = Some(self.repaint_after.map_or(after, |cur| cur.min(after)));
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
    /// The UI claims the pointer this frame: it is over a widget or a
    /// floating area, or a widget/drag is holding it. A host embedding the
    /// UI over a scene should not forward pointer input to the scene.
    pub wants_pointer: bool,
    /// Stronger than `wants_pointer`: the pointer is over a *floating*
    /// layer (popup/window), a pressed widget is holding it, or a
    /// drag-and-drop is active. A host stacking this UI over another UI
    /// toolkit (not just a scene) must withhold pointer events from the
    /// UI beneath while this is set, or clicks/drags on floating crusty
    /// windows leak through them.
    pub owns_pointer: bool,
    /// A widget has keyboard focus; the host should not treat keys as
    /// scene/game input.
    pub wants_keyboard: bool,
    /// Cursor the OS window should show, as requested by widgets this frame.
    pub cursor_icon: CursorIcon,
    /// Repaint scheduling hint: `Some(ZERO)` = repaint now (animating),
    /// `Some(d)` = repaint within `d`, `None` = idle until new input.
    pub repaint_after: Option<Duration>,
}

/// Backend-agnostic cursor shapes widgets can request via
/// [`Context::set_cursor_icon`]; the shell maps them to OS cursors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    #[default]
    Default,
    /// Hand with pointing finger (links, clickable chips).
    Pointer,
    /// I-beam over editable text.
    Text,
    /// Open hand: something grabbable (slider knob, pannable canvas).
    Grab,
    /// Closed hand while actively dragging.
    Grabbing,
    /// Horizontal resize (vertical splitter, column edge).
    ResizeEw,
    /// Vertical resize (horizontal splitter, row edge).
    ResizeNs,
    /// Diagonal ↘ resize (window corner).
    ResizeNwSe,
    /// Diagonal ↗ resize.
    ResizeNeSw,
    Move,
    NotAllowed,
    /// Precise picking (eyedropper, pixel selection).
    Crosshair,
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
    /// True while the most recent primary press was the second press of
    /// a double-click (quick + near the previous press). Persists until
    /// the next press, so a widget's release-click can test it:
    /// `response.clicked && press_was_double` = double-click.
    pub press_was_double: bool,
    /// Time of the previous primary press (double-click detection).
    last_press_time: f64,
    /// Position of the previous primary press (double-click detection).
    last_press_pos: Option<Pos2>,
    /// Secondary pointer button transitioned to "down" this frame
    /// (right-click — used to open context menus).
    pub right_pressed: bool,
    /// Middle pointer button is currently held (orbit-pan style drags).
    pub middle_down: bool,
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
    /// Mouse buttons that transitioned to "pressed" this frame,
    /// including non-primary buttons (middle, back, forward).
    pub button_presses: Vec<MouseButton>,
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
        self.button_presses.clear();
    }

    /// Classify this frame's press (if any) as the second of a
    /// double-click. Called once per frame after events are applied.
    fn detect_double_press(&mut self, now: f64) {
        const DOUBLE_CLICK_TIME: f64 = 0.35;
        const DOUBLE_CLICK_DIST: f32 = 6.0;

        if !self.pointer_pressed {
            return;
        }
        let near = match (self.pointer_pos, self.last_press_pos) {
            (Some(a), Some(b)) => {
                let (dx, dy) = (a.x - b.x, a.y - b.y);
                (dx * dx + dy * dy).sqrt() <= DOUBLE_CLICK_DIST
            }
            _ => false,
        };
        self.press_was_double = near && now - self.last_press_time <= DOUBLE_CLICK_TIME;
        if self.press_was_double {
            // A third quick click starts a fresh cycle instead of
            // chaining double-clicks.
            self.last_press_pos = None;
            self.last_press_time = f64::NEG_INFINITY;
        } else {
            self.last_press_pos = self.pointer_pos;
            self.last_press_time = now;
        }
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
                if *pressed {
                    self.button_presses.push(*button);
                }
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
                if matches!(button, MouseButton::Middle) {
                    self.middle_down = *pressed;
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

    /// True if `shortcut` (key + exact modifier set) was pressed this frame,
    /// without consuming it. Use [`consume_shortcut`](Self::consume_shortcut)
    /// when only one handler should fire.
    pub fn shortcut_pressed(&self, shortcut: crate::input::Shortcut) -> bool {
        self.key_presses
            .iter()
            .any(|k| k.key == shortcut.key && k.modifiers == shortcut.modifiers)
    }

    /// Like [`shortcut_pressed`](Self::shortcut_pressed) but removes the
    /// matching press so later handlers in the same frame won't also see it.
    pub fn consume_shortcut(&mut self, shortcut: crate::input::Shortcut) -> bool {
        if let Some(pos) = self
            .key_presses
            .iter()
            .position(|k| k.key == shortcut.key && k.modifiers == shortcut.modifiers)
        {
            self.key_presses.remove(pos);
            true
        } else {
            false
        }
    }
}
