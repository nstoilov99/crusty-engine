//! World streaming (M4): zone & chunk lifecycle driven by the M3 world
//! manifest. See `docs/roadmap/COMPLETED.md` ▸ Task M4.

pub mod lifecycle;
pub mod streamer;

pub use lifecycle::{StreamedCell, StreamingCtx, StreamingFrameOutput};
pub use streamer::{
    diff_ops, load_world, LoadedWorld, StreamOps, StreamingConfig, WorldLoadReport, WorldStreamer,
    ZoneChanged,
};
