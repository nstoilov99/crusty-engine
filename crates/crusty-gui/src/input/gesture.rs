//! Three-phase pointer gestures and wheel-notch accumulation.
//!
//! Rule 2 of the Crusty input model: **nothing commits on press**. A press only
//! arms a gesture; it becomes a drag when the pointer travels past
//! [`DRAG_THRESHOLD`], and if it never does, the release is a click. Keeping
//! the two outcomes on one tracker is what lets a button do double duty — the
//! right button pans when you drag it and opens a context menu when you don't.

use crate::math::Pos2;

/// Distance, in logical points, a pointer must travel after a press before the
/// gesture counts as a drag. Multiply by the host's UI scale for hit-testing in
/// a scaled editor — see [`drag_threshold`].
pub const DRAG_THRESHOLD: f32 = 4.0;

/// [`DRAG_THRESHOLD`] scaled for a host that renders its UI at `ui_scale`.
#[inline]
pub fn drag_threshold(ui_scale: f32) -> f32 {
    DRAG_THRESHOLD * ui_scale.max(0.1)
}

/// Where a tracked button gesture currently is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GesturePhase {
    /// No button held.
    #[default]
    Idle,
    /// Pressed, but still within the threshold — outcome undecided.
    Armed,
    /// Past the threshold; this is a drag and can no longer become a click.
    Dragging,
}

/// How a tracked gesture ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureEnd {
    /// Released without ever passing the threshold — a click, at this position.
    Click(Pos2),
    /// Released after dragging.
    Drag,
}

/// Three-phase tracker for one button: idle → armed → dragging.
///
/// Small and `Copy`, so it lives comfortably in widget `Memory` or in a host's
/// own state struct.
#[derive(Clone, Copy, Debug, Default)]
pub struct DragTracker {
    origin: Pos2,
    last: Pos2,
    phase: GesturePhase,
}

impl DragTracker {
    /// Arm the gesture at a press.
    pub fn press(&mut self, at: Pos2) {
        self.origin = at;
        self.last = at;
        self.phase = GesturePhase::Armed;
    }

    /// Feed the current pointer position. Returns `true` on the single frame
    /// the gesture crosses into [`GesturePhase::Dragging`].
    pub fn update(&mut self, at: Pos2, threshold: f32) -> bool {
        self.last = at;
        if self.phase != GesturePhase::Armed {
            return false;
        }
        let d = at - self.origin;
        if (d.x * d.x + d.y * d.y).sqrt() > threshold {
            self.phase = GesturePhase::Dragging;
            true
        } else {
            false
        }
    }

    /// End the gesture, reporting which of the two outcomes it was. `None` if
    /// it was never armed.
    pub fn release(&mut self) -> Option<GestureEnd> {
        let out = match self.phase {
            GesturePhase::Idle => None,
            // The click reports the *press* position, not the release: a click
            // means the pointer never meaningfully moved, and the press point
            // is where the user was looking.
            GesturePhase::Armed => Some(GestureEnd::Click(self.origin)),
            GesturePhase::Dragging => Some(GestureEnd::Drag),
        };
        self.phase = GesturePhase::Idle;
        out
    }

    /// Abandon the gesture with no outcome (Rule 3 — Escape, focus loss, a
    /// conflicting press).
    pub fn cancel(&mut self) {
        self.phase = GesturePhase::Idle;
    }

    #[inline]
    pub fn phase(&self) -> GesturePhase {
        self.phase
    }

    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.phase == GesturePhase::Dragging
    }

    /// Armed or dragging — i.e. the button is down and we are tracking it.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.phase != GesturePhase::Idle
    }

    #[inline]
    pub fn origin(&self) -> Pos2 {
        self.origin
    }

    #[inline]
    pub fn last(&self) -> Pos2 {
        self.last
    }
}

/// Turns a continuous scroll stream into whole wheel notches.
///
/// A mouse wheel reports one line-unit per detent, so it steps immediately; a
/// precision touchpad reports a fine-grained stream (normalised to the same
/// line units at the winit seam) that would otherwise fire hundreds of tiny
/// zoom steps per flick. Accumulating and only emitting whole notches — keeping
/// the residue — makes both devices produce the same discrete ladder.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollAccumulator {
    residue: f32,
}

impl ScrollAccumulator {
    /// Feed one frame's scroll delta; returns the whole notches crossed
    /// (signed, `0` while a touchpad is still building one up).
    pub fn feed(&mut self, delta: f32) -> i32 {
        if !delta.is_finite() {
            return 0;
        }
        self.residue += delta;
        let notches = self.residue.trunc();
        self.residue -= notches;
        notches as i32
    }

    /// Drop any partial notch — call when the gesture's target changes (the
    /// pointer left the canvas, the view was reset) so stale residue can't
    /// leak into the next flick.
    pub fn reset(&mut self) {
        self.residue = 0.0;
    }

    #[inline]
    pub fn residue(&self) -> f32 {
        self.residue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_that_barely_moves_is_a_click_not_a_drag() {
        let mut t = DragTracker::default();
        t.press(Pos2::new(10.0, 10.0));
        assert!(!t.update(Pos2::new(12.0, 12.0), DRAG_THRESHOLD));
        assert_eq!(t.phase(), GesturePhase::Armed);
        assert_eq!(t.release(), Some(GestureEnd::Click(Pos2::new(10.0, 10.0))));
    }

    #[test]
    fn crossing_the_threshold_latches_into_a_drag() {
        let mut t = DragTracker::default();
        t.press(Pos2::new(10.0, 10.0));
        assert!(t.update(Pos2::new(20.0, 10.0), DRAG_THRESHOLD));
        // Only the crossing frame reports true...
        assert!(!t.update(Pos2::new(30.0, 10.0), DRAG_THRESHOLD));
        // ...and coming back inside the threshold does not un-drag it.
        assert!(!t.update(Pos2::new(10.0, 10.0), DRAG_THRESHOLD));
        assert_eq!(t.phase(), GesturePhase::Dragging);
        assert_eq!(t.release(), Some(GestureEnd::Drag));
    }

    #[test]
    fn a_cancelled_gesture_reports_nothing() {
        let mut t = DragTracker::default();
        t.press(Pos2::ZERO);
        t.cancel();
        assert_eq!(t.release(), None);
    }

    #[test]
    fn the_threshold_scales_with_the_hosts_ui_scale() {
        assert!((drag_threshold(1.0) - DRAG_THRESHOLD).abs() < 1e-6);
        assert!((drag_threshold(1.5) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn a_wheel_notch_steps_at_once_but_touchpad_dust_accumulates() {
        let mut a = ScrollAccumulator::default();
        // Mouse wheel: one line unit per detent.
        assert_eq!(a.feed(1.0), 1);
        assert_eq!(a.feed(-1.0), -1);
        // Touchpad: twenty small deltas make exactly one notch, no more.
        let mut a = ScrollAccumulator::default();
        let steps: i32 = (0..20).map(|_| a.feed(0.05)).sum();
        assert_eq!(steps, 1);
        assert!(a.residue().abs() < 1e-3);
    }

    #[test]
    fn residue_is_kept_across_frames_and_never_lost() {
        let mut a = ScrollAccumulator::default();
        assert_eq!(a.feed(0.6), 0);
        assert_eq!(a.feed(0.6), 1);
        assert!((a.residue() - 0.2).abs() < 1e-5);
        a.reset();
        assert_eq!(a.residue(), 0.0);
    }
}
