//! winit → crusty-gui input translation.
//!
//! Free functions shared by the shell's window orchestration and by any
//! app-owned window that drives its own [`Context`](crate::context::Context).

use winit::event::WindowEvent;

use crate::input::{Event, Key, Modifiers, MouseButton};
use crate::math::{Pos2, Vec2};

/// Map a winit mouse button to ours.
pub fn map_mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Primary,
        winit::event::MouseButton::Right => MouseButton::Secondary,
        winit::event::MouseButton::Middle => MouseButton::Middle,
        _ => MouseButton::Other(0),
    }
}

/// Map winit modifier state to ours.
pub fn map_modifiers(mods: &winit::event::Modifiers) -> Modifiers {
    let s = mods.state();
    let mut m = Modifiers::empty();
    m.set(Modifiers::SHIFT, s.shift_key());
    m.set(Modifiers::CTRL, s.control_key());
    m.set(Modifiers::ALT, s.alt_key());
    m.set(Modifiers::META, s.super_key());
    m
}

/// Map a winit logical key to ours.
pub fn map_key(key: &winit::keyboard::Key) -> Key {
    use winit::keyboard::{Key as WKey, NamedKey};
    match key {
        WKey::Named(named) => match named {
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Enter => Key::Enter,
            NamedKey::Tab => Key::Tab,
            NamedKey::Escape => Key::Escape,
            NamedKey::Space => Key::Space,
            NamedKey::ArrowLeft => Key::ArrowLeft,
            NamedKey::ArrowRight => Key::ArrowRight,
            NamedKey::ArrowUp => Key::ArrowUp,
            NamedKey::ArrowDown => Key::ArrowDown,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Delete => Key::Delete,
            NamedKey::Insert => Key::Insert,
            _ => Key::Unknown,
        },
        WKey::Character(s) => s.chars().next().map(Key::Char).unwrap_or(Key::Unknown),
        _ => Key::Unknown,
    }
}

/// Map a winit scroll delta to a scroll vector (pixel deltas are normalised
/// to line-ish units).
pub fn map_scroll(delta: winit::event::MouseScrollDelta) -> Vec2 {
    use winit::event::MouseScrollDelta;
    match delta {
        MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
        MouseScrollDelta::PixelDelta(p) => Vec2::new((p.x as f32) / 20.0, (p.y as f32) / 20.0),
    }
}

/// Push the [`Event::Key`] (and, on press, [`Event::Text`]) for a winit
/// keyboard event.
pub fn push_key_event(
    key_event: &winit::event::KeyEvent,
    modifiers: Modifiers,
    pending_events: &mut Vec<Event>,
) {
    let pressed = matches!(key_event.state, winit::event::ElementState::Pressed);
    pending_events.push(Event::Key {
        key: map_key(&key_event.logical_key),
        pressed,
        repeat: key_event.repeat,
        modifiers,
    });
    if pressed {
        if let Some(text) = &key_event.text {
            pending_events.push(Event::Text(text.as_str().to_string()));
        }
    }
}

/// Translate one winit [`WindowEvent`] into queued [`Event`]s for a window's
/// next frame, updating its pointer position and the shared modifier state.
pub fn apply_input_event(
    event: WindowEvent,
    pointer_pos: &mut Option<Pos2>,
    pending_events: &mut Vec<Event>,
    modifiers: &mut Modifiers,
) {
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            let pos = Pos2::new(position.x as f32, position.y as f32);
            *pointer_pos = Some(pos);
            pending_events.push(Event::PointerMoved(pos));
        }
        WindowEvent::CursorLeft { .. } => {
            *pointer_pos = None;
            pending_events.push(Event::PointerGone);
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let pressed = matches!(state, winit::event::ElementState::Pressed);
            if let Some(pos) = *pointer_pos {
                pending_events.push(Event::PointerButton {
                    pos,
                    button: map_mouse_button(button),
                    pressed,
                    modifiers: *modifiers,
                });
            }
        }
        WindowEvent::ModifiersChanged(mods) => {
            *modifiers = map_modifiers(&mods);
        }
        WindowEvent::MouseWheel { delta, .. } => {
            pending_events.push(Event::Scroll(map_scroll(delta)));
        }
        WindowEvent::KeyboardInput { event: key_event, .. } => {
            push_key_event(&key_event, *modifiers, pending_events);
        }
        _ => {}
    }
}

/// Inject a synthetic primary-button release into a window's input queue so its
/// UI context registers the pointer coming up.
///
/// The drag-routing release arms (main tear-off, host title-drag, host tear-off)
/// consume the *real* release event themselves to resolve the drop, so it never
/// reaches [`apply_input_event`]. The window's `InputState` therefore keeps
/// `pointer_down == true`, and a press edge only fires on `pressed && !down` —
/// so the *next* real press is swallowed (it only flips `down` back), and the
/// drag/click finally registers on the second click. This is the "have to click
/// a tab twice to grab it after a tear-off / redock / upgrade" bug. The window a
/// lone tab is torn off from is the same one later promoted in place, so the
/// stuck state survives the role change — clear it here.
pub fn clear_stuck_pointer(
    pointer_pos: Option<Pos2>,
    pending_events: &mut Vec<Event>,
    modifiers: Modifiers,
) {
    pending_events.push(Event::PointerButton {
        pos: pointer_pos.unwrap_or(Pos2::ZERO),
        button: MouseButton::Primary,
        pressed: false,
        modifiers,
    });
}

/// True if the true screen-space cursor `g` falls within `window`'s outer
/// bounds (including OS chrome).
pub fn window_contains_global(window: &winit::window::Window, g: Pos2) -> bool {
    let Ok(pos) = window.outer_position() else {
        return false;
    };
    let size = window.outer_size();
    let x0 = pos.x as f32;
    let y0 = pos.y as f32;
    g.x >= x0 && g.y >= y0 && g.x < x0 + size.width as f32 && g.y < y0 + size.height as f32
}
