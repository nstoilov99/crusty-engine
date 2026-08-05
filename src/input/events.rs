use bitflags::bitflags;

use crate::math::{Pos2, Rect, Vec2};

/// One frame's worth of raw input plus per-frame parameters (screen rect, dpi,
/// elapsed time). The host fills this in and hands it to
/// [`Context::begin_frame`](crate::context::Context::begin_frame).
#[derive(Clone, Debug, Default)]
pub struct RawInput {
    pub screen_rect: Rect,
    pub pixels_per_point: f32,
    pub time: f64,
    pub predicted_dt: f32,
    pub modifiers: Modifiers,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug)]
pub enum Event {
    PointerMoved(Pos2),
    PointerButton {
        pos: Pos2,
        button: MouseButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    PointerGone,
    Scroll(Vec2),
    Text(String),
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Focused(bool),
    Resized(Rect),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    Other(u16),
}

bitflags! {
    // `Ord`/`Hash` so a (modifiers, key) chord can key a map — the editor's
    // keymap needs a total order to detect duplicate bindings.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct Modifiers: u8 {
        const SHIFT = 1 << 0;
        const CTRL  = 1 << 1;
        const ALT   = 1 << 2;
        // Windows key / Cmd on macOS.
        const META  = 1 << 3;
    }
}

/// A keyboard shortcut: a [`Key`] plus the exact modifier set that must be
/// held. Match against a frame's input with
/// [`InputState::consume_shortcut`](crate::context::InputState::consume_shortcut).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// `Ctrl` + `key`.
    pub const fn ctrl(key: Key) -> Self {
        Self::new(Modifiers::CTRL, key)
    }

    /// `Ctrl` + `Shift` + `key`.
    pub const fn ctrl_shift(key: Key) -> Self {
        Self::new(Modifiers::CTRL.union(Modifiers::SHIFT), key)
    }
}

/// Logical key codes. Mirrors the subset of `winit::keyboard::Key` that
/// crusty-gui cares about; the host translates from `winit` into this.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Key {
    Escape,
    Tab,
    Backspace,
    Enter,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    Char(char),
    Unknown,
}
