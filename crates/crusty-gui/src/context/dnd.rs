//! A small, type-erased drag-and-drop payload channel.
//!
//! A drag source stashes a boxed payload + a display label when the user
//! starts dragging; the payload rides along (surviving across frames) until
//! a drop target of the matching type takes it, or the drag is cancelled on
//! release over empty space. See [`Ui::dnd_drag_source`](crate::context::Ui::dnd_drag_source)
//! and [`Ui::dnd_drop_target`](crate::context::Ui::dnd_drop_target).

use std::any::{Any, TypeId};

/// Carries the in-flight drag payload. Lives on the [`Context`](crate::context::Context).
#[derive(Default)]
pub struct DragDrop {
    payload: Option<Box<dyn Any>>,
    type_id: Option<TypeId>,
    /// Short text shown in the drag ghost.
    label: String,
    /// True while a payload is being carried (set once the drag activates).
    dragging: bool,
}

impl DragDrop {
    /// Begin carrying `payload` (called by a drag source on activation).
    pub fn begin(&mut self, payload: Box<dyn Any>, type_id: TypeId, label: String) {
        self.payload = Some(payload);
        self.type_id = Some(type_id);
        self.label = label;
        self.dragging = true;
    }

    /// True while a payload is in flight.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// True if the in-flight payload is exactly of type `T`.
    pub fn holds<T: 'static>(&self) -> bool {
        self.type_id == Some(TypeId::of::<T>())
    }

    /// The drag ghost label, if a drag is active.
    pub fn label(&self) -> Option<&str> {
        self.dragging.then_some(self.label.as_str())
    }

    /// Borrow the in-flight payload if it is a `T`, without ending the
    /// drag. Lets a drop target inspect the payload mid-drag (e.g. to
    /// refuse dropping a tree node onto itself).
    pub fn peek<T: 'static>(&self) -> Option<&T> {
        self.payload.as_ref()?.downcast_ref::<T>()
    }

    /// Take the payload if it is a `T`, ending the drag. Returns `None`
    /// (leaving the drag intact) on a type mismatch.
    pub fn take<T: 'static>(&mut self) -> Option<T> {
        if !self.holds::<T>() {
            return None;
        }
        let boxed = self.payload.take()?;
        self.type_id = None;
        self.dragging = false;
        boxed.downcast::<T>().ok().map(|b| *b)
    }

    /// Drop the in-flight payload (drag cancelled).
    pub fn clear(&mut self) {
        self.payload = None;
        self.type_id = None;
        self.label.clear();
        self.dragging = false;
    }
}
