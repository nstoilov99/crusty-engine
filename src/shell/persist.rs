//! Dock-layout persistence: serializes the dock tree alongside the
//! [`Context`](crate::context::Context)'s [`Memory`] (window positions, scroll
//! offsets, animation state) so a relaunch restores both the panel arrangement
//! and the UI state.

use std::path::Path;

use crate::context::Memory;
use crate::dock::DockNode;

/// Borrowed view written to disk.
#[derive(serde::Serialize)]
struct LayoutRef<'a> {
    dock: &'a DockNode,
    memory: &'a Memory,
}

/// Owned counterpart read back from disk.
#[derive(serde::Deserialize)]
struct LayoutOwned {
    dock: DockNode,
    memory: Memory,
}

/// Restore a dock tree + UI memory from `path`. Returns `None` (with a log
/// line) if the file is missing or stale — callers keep their defaults.
pub fn load_layout(path: &Path) -> Option<(DockNode, Memory)> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<LayoutOwned>(&text) {
        Ok(layout) => {
            log::info!("restored dock layout from {}", path.display());
            Some((layout.dock, layout.memory))
        }
        Err(e) => {
            log::warn!("ignoring unreadable {}: {e}", path.display());
            None
        }
    }
}

/// Write the dock tree + UI memory to `path`.
pub fn save_layout(path: &Path, dock: &DockNode, memory: &Memory) {
    let layout = LayoutRef { dock, memory };
    match serde_json::to_string_pretty(&layout) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                log::warn!("failed writing {}: {e}", path.display());
            }
        }
        Err(e) => log::warn!("failed serializing layout: {e}"),
    }
}
