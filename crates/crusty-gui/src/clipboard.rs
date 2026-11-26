//! Thin wrapper over [`arboard`] for system clipboard access.
//!
//! A fresh [`arboard::Clipboard`] is opened per call. That's a touch
//! heavier than caching one, but clipboard ops only happen on explicit
//! Ctrl+C / Ctrl+V, so the simplicity is worth it — and it sidesteps
//! threading a clipboard handle through `Context`.

use arboard::Clipboard;

/// Read UTF-8 text from the system clipboard, or `None` if it's empty,
/// non-text, or the platform clipboard couldn't be opened.
pub fn get_text() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

/// Write `text` to the system clipboard. Failures are swallowed — a
/// missing clipboard shouldn't crash the UI.
pub fn set_text(text: impl Into<String>) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text.into());
    }
}
