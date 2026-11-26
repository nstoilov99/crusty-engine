//! Headless unit tests for input semantics: press/release edge flags,
//! double-click detection, per-frame event lists and their reset. Pure
//! `Context` frames — no GPU, no text renderer.

use crusty_gui::context::Context;
use crusty_gui::input::{Event, Key, Modifiers, MouseButton, RawInput};
use crusty_gui::math::{Pos2, Rect, Vec2};

fn input_at(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
        pixels_per_point: 1.0,
        time,
        predicted_dt: 1.0 / 60.0,
        modifiers: Modifiers::empty(),
        events,
    }
}

fn button(pos: Pos2, button: MouseButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::empty(),
    }
}

fn primary(pos: Pos2, pressed: bool) -> Event {
    button(pos, MouseButton::Primary, pressed)
}

#[test]
fn pointer_press_and_release_edges() {
    let mut ctx = Context::new();
    let p = Pos2::new(10.0, 10.0);

    ctx.begin_frame(input_at(0.0, vec![primary(p, true)]));
    assert!(
        ctx.input.pointer_pressed,
        "press edge fires on the press frame"
    );
    assert!(ctx.input.pointer_down);
    assert!(!ctx.input.pointer_released);
    ctx.end_frame();

    ctx.begin_frame(input_at(1.0, vec![]));
    assert!(!ctx.input.pointer_pressed, "press edge resets next frame");
    assert!(ctx.input.pointer_down, "held flag persists");
    ctx.end_frame();

    ctx.begin_frame(input_at(2.0, vec![primary(p, false)]));
    assert!(ctx.input.pointer_released);
    assert!(!ctx.input.pointer_down);
    ctx.end_frame();
}

#[test]
fn double_click_needs_quick_and_near_presses() {
    let mut ctx = Context::new();
    let p = Pos2::new(50.0, 50.0);

    // First press: never a double.
    ctx.begin_frame(input_at(0.00, vec![primary(p, true)]));
    assert!(!ctx.input.press_was_double);
    ctx.end_frame();
    ctx.begin_frame(input_at(0.05, vec![primary(p, false)]));
    ctx.end_frame();

    // Second quick press nearby: double.
    ctx.begin_frame(input_at(0.15, vec![primary(Pos2::new(52.0, 51.0), true)]));
    assert!(ctx.input.press_was_double);
    ctx.end_frame();
    ctx.begin_frame(input_at(0.20, vec![primary(p, false)]));
    ctx.end_frame();

    // Third quick press starts a fresh cycle instead of chaining.
    ctx.begin_frame(input_at(0.30, vec![primary(p, true)]));
    assert!(!ctx.input.press_was_double);
    ctx.end_frame();
}

#[test]
fn slow_or_far_second_press_is_not_a_double_click() {
    let mut ctx = Context::new();
    let p = Pos2::new(50.0, 50.0);

    // Too slow.
    ctx.begin_frame(input_at(0.0, vec![primary(p, true)]));
    ctx.end_frame();
    ctx.begin_frame(input_at(0.1, vec![primary(p, false)]));
    ctx.end_frame();
    ctx.begin_frame(input_at(1.0, vec![primary(p, true)]));
    assert!(
        !ctx.input.press_was_double,
        "second press after 1s is not a double"
    );
    ctx.end_frame();
    ctx.begin_frame(input_at(1.1, vec![primary(p, false)]));
    ctx.end_frame();

    // Quick but far away.
    ctx.begin_frame(input_at(1.2, vec![primary(Pos2::new(200.0, 200.0), true)]));
    assert!(
        !ctx.input.press_was_double,
        "second press 150px away is not a double"
    );
    ctx.end_frame();
}

#[test]
fn typed_text_and_key_presses_reset_each_frame() {
    let mut ctx = Context::new();

    ctx.begin_frame(input_at(
        0.0,
        vec![
            Event::Text("ab".into()),
            Event::Text("c".into()),
            Event::Key {
                key: Key::Enter,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL,
            },
            Event::Key {
                key: Key::Escape,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::empty(),
            },
        ],
    ));
    assert_eq!(
        ctx.input.typed_text, "abc",
        "text events concatenate within a frame"
    );
    assert_eq!(
        ctx.input.key_presses.len(),
        1,
        "key releases are not recorded"
    );
    assert!(matches!(ctx.input.key_presses[0].key, Key::Enter));
    assert!(ctx.input.key_presses[0].modifiers.contains(Modifiers::CTRL));
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![]));
    assert!(ctx.input.typed_text.is_empty());
    assert!(ctx.input.key_presses.is_empty());
    ctx.end_frame();
}

#[test]
fn scroll_accumulates_within_a_frame_and_resets() {
    let mut ctx = Context::new();

    ctx.begin_frame(input_at(
        0.0,
        vec![
            Event::Scroll(Vec2::new(0.0, -30.0)),
            Event::Scroll(Vec2::new(10.0, -10.0)),
        ],
    ));
    assert_eq!(ctx.input.scroll_delta, Vec2::new(10.0, -40.0));
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![]));
    assert_eq!(ctx.input.scroll_delta, Vec2::ZERO);
    ctx.end_frame();
}

#[test]
fn secondary_and_middle_buttons() {
    let mut ctx = Context::new();
    let p = Pos2::new(10.0, 10.0);

    ctx.begin_frame(input_at(
        0.0,
        vec![
            button(p, MouseButton::Secondary, true),
            button(p, MouseButton::Middle, true),
        ],
    ));
    assert!(ctx.input.right_pressed);
    assert!(ctx.input.middle_down);
    assert!(
        !ctx.input.pointer_pressed,
        "non-primary buttons don't set the primary press edge"
    );
    assert_eq!(
        ctx.input.button_presses,
        vec![MouseButton::Secondary, MouseButton::Middle]
    );
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![]));
    assert!(!ctx.input.right_pressed, "right press edge resets");
    assert!(ctx.input.middle_down, "middle held flag persists");
    assert!(ctx.input.button_presses.is_empty());
    ctx.end_frame();

    ctx.begin_frame(input_at(0.2, vec![button(p, MouseButton::Middle, false)]));
    assert!(!ctx.input.middle_down);
    ctx.end_frame();
}

#[test]
fn modifiers_come_from_raw_input() {
    let mut ctx = Context::new();
    let mut raw = input_at(0.0, vec![]);
    raw.modifiers = Modifiers::SHIFT | Modifiers::ALT;
    ctx.begin_frame(raw);
    assert!(
        ctx.input
            .modifiers
            .contains(Modifiers::SHIFT | Modifiers::ALT)
    );
    assert!(!ctx.input.modifiers.contains(Modifiers::CTRL));
    ctx.end_frame();
}

#[test]
fn click_clears_keyboard_focus() {
    let mut ctx = Context::new();
    let id = crusty_gui::id::Id::new("field");
    ctx.memory.focused_widget = Some(id);

    ctx.begin_frame(input_at(0.0, vec![]));
    assert_eq!(
        ctx.memory.focused_widget,
        Some(id),
        "focus persists without clicks"
    );
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![primary(Pos2::new(5.0, 5.0), true)]));
    assert_eq!(
        ctx.memory.focused_widget, None,
        "a press anywhere clears focus (widgets re-claim during the frame)"
    );
    ctx.end_frame();
}

fn tab(shift: bool) -> Event {
    Event::Key {
        key: Key::Tab,
        pressed: true,
        repeat: false,
        modifiers: if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::empty()
        },
    }
}

#[test]
fn tab_cycles_focus_in_registration_order() {
    let mut ctx = Context::new();
    let ids: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(crusty_gui::id::Id::new)
        .collect();

    // Tab with nothing focused lands on the first widget.
    ctx.begin_frame(input_at(0.0, vec![tab(false)]));
    for id in &ids {
        ctx.memory.register_focusable(*id, None);
    }
    ctx.end_frame();
    assert_eq!(ctx.memory.focused_widget, Some(ids[0]));

    // Next Tab advances; wrap after the last.
    for expected in [ids[1], ids[2], ids[0]] {
        ctx.begin_frame(input_at(0.1, vec![tab(false)]));
        for id in &ids {
            ctx.memory.register_focusable(*id, None);
        }
        ctx.end_frame();
        assert_eq!(ctx.memory.focused_widget, Some(expected));
    }

    // Shift-Tab goes back.
    ctx.begin_frame(input_at(0.2, vec![tab(true)]));
    for id in &ids {
        ctx.memory.register_focusable(*id, None);
    }
    ctx.end_frame();
    assert_eq!(ctx.memory.focused_widget, Some(ids[2]));
}

#[test]
fn tab_traversal_stays_within_the_focused_scope() {
    let mut ctx = Context::new();
    let root_a = crusty_gui::id::Id::new("root_a");
    let root_b = crusty_gui::id::Id::new("root_b");
    let win = crusty_gui::id::Id::new("win");
    let win_a = crusty_gui::id::Id::new("win_a");
    let win_b = crusty_gui::id::Id::new("win_b");
    ctx.memory.focused_widget = Some(win_a);

    ctx.begin_frame(input_at(0.0, vec![tab(false)]));
    ctx.memory.register_focusable(root_a, None);
    ctx.memory.register_focusable(win_a, Some(win));
    ctx.memory.register_focusable(win_b, Some(win));
    ctx.memory.register_focusable(root_b, None);
    ctx.end_frame();
    assert_eq!(
        ctx.memory.focused_widget,
        Some(win_b),
        "Tab from a window widget skips root widgets"
    );

    // Wraps within the window scope, never escaping to root.
    ctx.begin_frame(input_at(0.1, vec![tab(false)]));
    ctx.memory.register_focusable(root_a, None);
    ctx.memory.register_focusable(win_a, Some(win));
    ctx.memory.register_focusable(win_b, Some(win));
    ctx.memory.register_focusable(root_b, None);
    ctx.end_frame();
    assert_eq!(ctx.memory.focused_widget, Some(win_a));
}

#[test]
fn widget_focus_requests_move_and_enter_scope() {
    let mut ctx = Context::new();
    let a = crusty_gui::id::Id::new("a");
    let b = crusty_gui::id::Id::new("b");
    let popup = crusty_gui::id::Id::new("popup");
    let row = crusty_gui::id::Id::new("row");
    ctx.memory.focused_widget = Some(a);

    // Arrow-key style request: step forward without a Tab press.
    ctx.begin_frame(input_at(0.0, vec![]));
    ctx.memory.register_focusable(a, None);
    ctx.memory.register_focusable(b, None);
    ctx.memory.focus_move_requested = Some(false);
    ctx.end_frame();
    assert_eq!(ctx.memory.focused_widget, Some(b));

    // Scope request (combo ArrowDown): first widget of the popup scope.
    ctx.begin_frame(input_at(0.1, vec![]));
    ctx.memory.register_focusable(a, None);
    ctx.memory.register_focusable(row, Some(popup));
    ctx.memory.focus_scope_requested = Some(popup);
    ctx.end_frame();
    assert_eq!(ctx.memory.focused_widget, Some(row));
}
