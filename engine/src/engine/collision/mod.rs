//! Static-collision cooking (offline) — see
//! `docs/roadmap/COMPLETED.md` ▸ Task M2.
//!
//! Runtime loading/querying lives in `game_shared::collision`; this module
//! only produces cooked `.ccol` chunks from a scene.

pub mod cook;
pub mod output;
pub mod world;

pub use world::{CollisionLoadReport, CollisionWorld};
