//! Render backends. Phase 0 reserves the module; the vulkano backend lands
//! in Phase 1 once paint commands and the tessellator exist.

#[cfg(feature = "vulkano-renderer")]
pub mod vulkano;
