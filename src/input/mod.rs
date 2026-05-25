//! Raw input collected by the host (windowing layer) and handed to the
//! [`Context`](crate::context::Context) at the start of every frame.
//!
//! Phase 0 only declares the data shapes — routing into widgets lands in
//! Phase 3 together with the retained tree.

mod events;

pub use events::{Event, Key, Modifiers, MouseButton, RawInput};
