//! Per-widget persistent state keyed by [`Id`]. Lives across frames so
//! widgets can hit-test against last frame's rect, animate smoothly, and
//! remember focus/scroll/etc.
//!
//! Phase 3 stores only what the Label/Button widgets need. The map is
//! garbage-collected once per frame: entries not touched by the current
//! frame's UI calls get dropped after a grace period.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use ahash::AHasher;

use crate::id::Id;
use crate::layer::Order;
use crate::math::{Pos2, Rect, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct WidgetMemory {
    /// Rect the widget occupied last time it was called. Used for hit
    /// testing in the *current* frame.
    pub last_rect: Rect,
    pub hover_anim: f32,
    pub press_anim: f32,
    /// Widget-specific extra animation value (semantics depend on the
    /// widget that owns the Id): Checkbox/Radio = check-fade,
    /// ProgressBar = smoothed display value, CollapsingHeader = openness
    /// animation, ScrollArea = remembered total content height.
    pub aux_anim: f32,
    /// Widget-specific boolean — e.g. CollapsingHeader's open/closed
    /// state, TextEdit's focus across frames if needed.
    pub bool_state: bool,
    /// Widget-specific 2D value — e.g. ScrollArea's scroll offset.
    pub aux_vec2: Vec2,
    /// Widget-specific unsigned int — e.g. TextEdit's cursor position.
    pub aux_usize: usize,
    /// True if this id was visited during the current frame's UI pass.
    /// Reset to false at frame start; checked at frame end for GC.
    pub seen_this_frame: bool,
    /// Frames since this widget was last seen — anything > a few frames
    /// gets evicted.
    pub frames_unseen: u8,
}

impl Default for WidgetMemory {
    fn default() -> Self {
        Self {
            last_rect: Rect::ZERO,
            hover_anim: 0.0,
            press_anim: 0.0,
            aux_anim: 0.0,
            bool_state: false,
            aux_vec2: Vec2::ZERO,
            aux_usize: 0,
            seen_this_frame: false,
            frames_unseen: 0,
        }
    }
}

/// Persistent state for a floating region (Area / Window / popup).
/// `pos`/`size` describe the rect the area occupied last frame and drive
/// pointer-priority hit testing for the *current* frame.
#[derive(Clone, Copy, Debug)]
pub struct AreaState {
    pub pos: Pos2,
    pub size: Vec2,
    pub order: Order,
    /// Monotonic stacking counter; higher = in front within the same
    /// [`Order`].
    pub z: u64,
    /// Non-interactable areas (tooltips) never claim the pointer.
    pub interactable: bool,
    pub seen_this_frame: bool,
    pub frames_unseen: u8,
}

impl AreaState {
    pub fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.size)
    }
}

#[derive(Default)]
pub struct Memory {
    widgets: HashMap<Id, WidgetMemory, BuildHasherDefault<AHasher>>,
    areas: HashMap<Id, AreaState, BuildHasherDefault<AHasher>>,
    /// Next value handed out by [`bring_area_to_front`](Self::bring_area_to_front).
    next_area_z: u64,
    /// Whichever widget the user is currently pressing the primary
    /// pointer button on. Set when a press starts over a widget, cleared
    /// when the button is released. Used so a press that drifts off the
    /// widget still counts as "this widget is being pressed".
    pub active_widget: Option<Id>,
    /// Widget that has keyboard focus (typed text + keys route here).
    /// Cleared at the start of any frame with a pointer press; the
    /// just-clicked widget gets to re-claim it.
    pub focused_widget: Option<Id>,
    /// Set by a popup child (e.g. `SelectableValue` inside a `ComboBox`
    /// dropdown) to ask its parent popup to close on the current frame.
    /// Cleared at the start of every frame.
    pub popup_close_requested: bool,
}

impl Memory {
    pub fn get(&self, id: Id) -> Option<&WidgetMemory> {
        self.widgets.get(&id)
    }

    pub fn get_or_default(&mut self, id: Id) -> &mut WidgetMemory {
        self.widgets.entry(id).or_default()
    }

    pub fn area(&self, id: Id) -> Option<&AreaState> {
        self.areas.get(&id)
    }

    /// Fetch (or create, front-most) the persistent state for a floating
    /// area, marking it alive for this frame.
    pub fn area_entry(&mut self, id: Id, default_pos: Pos2, order: Order) -> &mut AreaState {
        let next_z = &mut self.next_area_z;
        let entry = self.areas.entry(id).or_insert_with(|| {
            let z = *next_z;
            *next_z += 1;
            AreaState {
                pos: default_pos,
                size: Vec2::ZERO,
                order,
                z,
                interactable: true,
                seen_this_frame: false,
                frames_unseen: 0,
            }
        });
        entry.order = order;
        entry.seen_this_frame = true;
        entry
    }

    /// Raise `id` above every other area in its order class.
    pub fn bring_area_to_front(&mut self, id: Id) {
        let z = self.next_area_z;
        if let Some(area) = self.areas.get_mut(&id) {
            area.z = z;
            self.next_area_z += 1;
        }
    }

    /// True when `id` is the front-most area of its order class.
    pub fn is_area_front(&self, id: Id) -> bool {
        let Some(area) = self.areas.get(&id) else {
            return false;
        };
        self.areas
            .values()
            .filter(|a| a.order == area.order)
            .all(|a| a.z <= area.z)
    }

    /// Topmost interactable area at `pointer` using last-frame rects, or
    /// the front-most modal when the pointer is outside every modal-or-
    /// higher area (a modal blocks everything below it). `None` means the
    /// root UI owns the pointer.
    ///
    /// Must be called *before* [`begin_frame`](Self::begin_frame) clears
    /// the `seen_this_frame` flags: only areas alive last frame count.
    /// Area entries themselves are never evicted so closed windows keep
    /// their position/size for when they reopen.
    pub fn hovered_area(&self, pointer: Option<Pos2>) -> Option<Id> {
        let mut top: Option<(Order, u64, Id)> = None;
        let mut front_modal: Option<(u64, Id)> = None;
        for (&id, a) in &self.areas {
            if !a.interactable || !a.seen_this_frame {
                continue;
            }
            if a.order == Order::Modal && front_modal.is_none_or(|(z, _)| a.z > z) {
                front_modal = Some((a.z, id));
            }
            if let Some(p) = pointer {
                if a.rect().contains(p) && top.is_none_or(|(o, z, _)| (a.order, a.z) > (o, z)) {
                    top = Some((a.order, a.z, id));
                }
            }
        }
        match (top, front_modal) {
            (Some((order, _, id)), Some((_, modal_id))) => {
                if order >= Order::Modal {
                    Some(id)
                } else {
                    Some(modal_id)
                }
            }
            (Some((_, _, id)), None) => Some(id),
            (None, Some((_, modal_id))) => Some(modal_id),
            (None, None) => None,
        }
    }

    /// Run at the start of each frame, before any widget code runs.
    /// Marks every entry as not-yet-seen so we can detect dead widgets.
    pub fn begin_frame(&mut self) {
        for entry in self.widgets.values_mut() {
            entry.seen_this_frame = false;
        }
        for area in self.areas.values_mut() {
            area.seen_this_frame = false;
        }
    }

    /// Run at the end of each frame, after all widget code has run.
    /// Drops entries that haven't been seen for more than `keep_for`
    /// frames (a small grace window so transiently-hidden widgets
    /// don't lose their animation state immediately).
    pub fn end_frame(&mut self, keep_for: u8) {
        self.widgets.retain(|_, entry| {
            if entry.seen_this_frame {
                entry.frames_unseen = 0;
                true
            } else {
                entry.frames_unseen = entry.frames_unseen.saturating_add(1);
                entry.frames_unseen <= keep_for
            }
        });
    }
}
