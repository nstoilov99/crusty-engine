//! Modal layer stack — one place that decides when a transient surface dies.
//!
//! Context menus, popups, combo dropdowns and menu flyouts all used to carry
//! their own copy of "did the user press outside me / hit Escape / run a
//! command", and each copy was subtly different: some checked
//! `topmost_area_contains` so a nested child popup would not collapse its
//! parent, some did not; none of them stopped the dismissing press from *also*
//! landing on whatever sat underneath, so closing a menu could select a node.
//!
//! [`ModalStack`] replaces all of that. A surface registers itself every frame
//! it draws ([`Context::modal_push`]); dismissal is resolved once, in
//! `begin_frame`, against the rects registered *last* frame — the same
//! one-frame-behind trick [`Memory::hovered_area`] uses, and for the same
//! reason: the decision has to be made before any widget reads the input.
//!
//! The rules (Rule 1 of the Crusty input model):
//!
//! - a press outside the top surface dismisses it **and is consumed**, so no
//!   widget beneath ever sees it;
//! - `Escape` pops one surface;
//! - a wheel outside dismisses the whole stack;
//! - losing window focus (or minimizing) dismisses the whole stack;
//! - running a command from inside dismisses (via `popup_close_requested`).
//!
//! One exception, because the intent is unambiguous: a *right* press outside
//! dismisses and is consumed like any other, but records where it landed in
//! [`ModalStack::reopen_at`] so the surface's owner can open a fresh context
//! menu there in the same frame instead of making the user click twice.

use crate::id::Id;
use crate::input::Key;
use crate::math::{Pos2, Rect, Vec2};

use super::InputState;

/// One transient surface, as registered by the widget that draws it.
#[derive(Clone, Copy, Debug)]
struct ModalEntry {
    id: Id,
    rect: Rect,
    /// Frame index this entry was last refreshed on. A surface that stops
    /// drawing (its owner closed it, or the panel went away) stops refreshing,
    /// and the entry is dropped rather than lingering as a phantom hit target.
    frame: u64,
}

/// Why the top of the stack went away this frame. Surfaces read this to decide
/// whether to clear their own open flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DismissCause {
    /// A pointer press landed outside.
    PressOutside,
    /// `Escape`.
    Escape,
    /// A wheel/scroll landed outside.
    ScrollOutside,
    /// The window lost focus or was minimized.
    FocusLost,
}

/// Stack of open transient surfaces, innermost last.
#[derive(Default)]
pub struct ModalStack {
    entries: Vec<ModalEntry>,
    /// Surfaces dismissed during this frame's `begin_frame`, with the reason.
    /// Cleared at the start of every frame.
    dismissed: Vec<(Id, DismissCause)>,
    /// Set when the dismissing press was a *right* press: where it landed.
    reopen_at: Option<Pos2>,
}

impl ModalStack {
    /// Register (or refresh) a surface for this frame. Order of calls within a
    /// frame is the stack order, so a child popup drawn inside a menu lands
    /// above its parent and shields it from the outside-press rule.
    pub(crate) fn push(&mut self, id: Id, rect: Rect, frame: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.rect = rect;
            e.frame = frame;
        } else {
            self.entries.push(ModalEntry { id, rect, frame });
        }
    }

    /// Drop a surface and everything stacked above it. Used when the owner
    /// closes itself (a menu item ran a command).
    pub(crate) fn remove(&mut self, id: Id) {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            self.entries.truncate(i);
        }
    }

    pub(crate) fn is_open(&self, id: Id) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    pub(crate) fn top(&self) -> Option<Id> {
        self.entries.last().map(|e| e.id)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Did `id` get dismissed by this frame's resolve step?
    pub(crate) fn was_dismissed(&self, id: Id) -> Option<DismissCause> {
        self.dismissed.iter().find(|(i, _)| *i == id).map(|(_, c)| *c)
    }

    pub(crate) fn reopen_at(&self) -> Option<Pos2> {
        self.reopen_at
    }

    pub(crate) fn take_reopen(&mut self) -> Option<Pos2> {
        self.reopen_at.take()
    }

    /// Does `p` fall inside any registered surface? Lets a host ask "is the
    /// pointer over a transient surface" without knowing which one.
    pub(crate) fn contains(&self, p: Pos2) -> bool {
        self.entries.iter().any(|e| e.rect.contains(p))
    }

    /// Does `p` fall inside a surface stacked strictly **above** `id`? A
    /// surface hosting interactive content (an embedded canvas) asks this
    /// instead of [`contains`](Self::contains): its own rect must not claim
    /// pointer input away from the widgets it hosts, while a popup it opened
    /// on top still does. An `id` not on the stack answers like `contains`.
    pub(crate) fn contains_above(&self, id: Id, p: Pos2) -> bool {
        let start = self
            .entries
            .iter()
            .position(|e| e.id == id)
            .map_or(0, |i| i + 1);
        self.entries[start..].iter().any(|e| e.rect.contains(p))
    }

    /// Is any surface stacked strictly **above** `id` open? The keyboard
    /// counterpart of [`contains_above`](Self::contains_above): a surface
    /// hosting shortcuts stays live while it is the top of the stack, and
    /// yields the moment something opens over it. An `id` not on the stack
    /// answers like "is anything open at all".
    pub(crate) fn any_above(&self, id: Id) -> bool {
        match self.entries.iter().position(|e| e.id == id) {
            Some(i) => i + 1 < self.entries.len(),
            None => !self.entries.is_empty(),
        }
    }

    fn dismiss_from(&mut self, cut: usize, cause: DismissCause) {
        for e in self.entries.drain(cut..) {
            self.dismissed.push((e.id, cause));
        }
    }

    /// The whole dismissal decision, run once per frame from `begin_frame`
    /// before any widget reads the input. Consumes whatever it acts on.
    pub(crate) fn resolve(&mut self, input: &mut InputState, focus_lost: bool, frame: u64) {
        self.dismissed.clear();
        self.reopen_at = None;
        // A surface that stopped drawing last frame is gone; do not keep
        // shielding presses on its behalf.
        self.entries.retain(|e| frame.saturating_sub(e.frame) <= 1);
        if self.entries.is_empty() {
            return;
        }

        if focus_lost {
            self.dismiss_from(0, DismissCause::FocusLost);
            return;
        }

        if input.key_pressed(Key::Escape) {
            input.consume_key(Key::Escape);
            let cut = self.entries.len() - 1;
            self.dismiss_from(cut, DismissCause::Escape);
            return;
        }

        let Some(p) = input.pointer_pos else { return };

        if input.pointer_pressed || input.right_pressed {
            // Keep every surface up to and including the deepest one under the
            // pointer; everything above it is "outside".
            let cut = self
                .entries
                .iter()
                .rposition(|e| e.rect.contains(p))
                .map_or(0, |i| i + 1);
            if cut < self.entries.len() {
                if input.right_pressed {
                    // The unambiguous-intent exception: let the owner reopen here.
                    self.reopen_at = Some(p);
                }
                self.dismiss_from(cut, DismissCause::PressOutside);
                input.consume_press();
            }
            return;
        }

        if input.scroll_delta != Vec2::ZERO && !self.contains(p) {
            self.dismiss_from(0, DismissCause::ScrollOutside);
            input.scroll_delta = Vec2::ZERO;
        }
    }
}
