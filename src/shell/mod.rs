//! Application shell: reusable winit/vulkano orchestration for multi-window
//! docking apps — the `eframe` to crusty-gui's `egui`.
//!
//! Phase 15.5 extraction in progress: input translation lives here; the
//! multi-window dock host / tear-off orchestration follows.

pub mod drag;
pub mod host;
pub mod input;
#[cfg(feature = "serde")]
pub mod persist;

