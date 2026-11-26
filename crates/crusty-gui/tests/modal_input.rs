//! Rule 1 (modal layer stack) and Rule 2 (three-phase gestures).
//!
//! These are the regression tests for the reported input bugs; they were
//! written before the fix and are named for the behaviour, not the mechanism.
//!
//! - **T1** a press that closes a context menu must not also act on whatever
//!   was under it.
//! - **T3** a wheel outside an open menu closes it.
//! - **T5** right-press, small move, release → context menu.
//! - **T6** right-press, big move, release → the canvas panned and *no* menu.

#[macro_use]
mod common;

use common::Harness;

use crusty_gui::context::Context;
use crusty_gui::input::{Event, Key, Modifiers, MouseButton, RawInput};
use crusty_gui::math::{Pos2, Rect, Vec2};
use crusty_gui::widgets::{Canvas, CanvasView};

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

fn button(pos: Pos2, b: MouseButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: b,
        pressed,
        modifiers: Modifiers::empty(),
    }
}

fn key(k: Key, pressed: bool) -> Event {
    Event::Key {
        key: k,
        pressed,
        repeat: false,
        modifiers: Modifiers::empty(),
    }
}

/// A stand-in for an open context menu: register a surface every frame the
/// caller considers it open, exactly as `context_menu_at` does.
fn open_menu(ctx: &mut Context, rect: Rect) {
    ctx.modal_push(crusty_gui::id::Id::ROOT.with("menu"), rect);
}

fn menu_id() -> crusty_gui::id::Id {
    crusty_gui::id::Id::ROOT.with("menu")
}

// ─────────────────────────────────────────────────────────────────────────────
// T1 — the dismissing press is consumed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn t1_a_press_that_closes_a_menu_is_consumed() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let outside = Pos2::new(50.0, 50.0);

    let mut ctx = Context::new();
    // Frame 1: the menu is open and registers itself.
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(outside)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    // Frame 2: press well outside it.
    ctx.begin_frame(input_at(
        0.1,
        vec![button(outside, MouseButton::Primary, true)],
    ));
    assert_eq!(
        ctx.modal_dismissed(menu_id()),
        Some(crusty_gui::context::DismissCause::PressOutside),
        "the menu closes"
    );
    assert!(
        !ctx.input.pointer_pressed,
        "and the press is swallowed, so nothing beneath it can act on the same click"
    );
    ctx.end_frame();
}

#[test]
fn t1_the_widget_under_a_menu_is_not_clicked() {
    // The same rule, end to end: a button sitting under an open context menu
    // must not fire on the click that dismisses the menu.
    let mut h = harness_or_skip!([400, 300]);
    let menu = Rect::from_min_size(Pos2::new(200.0, 20.0), Vec2::new(180.0, 120.0));
    let on_button = Pos2::new(40.0, 24.0);

    let mut clicked = false;
    h.frame(vec![Event::PointerMoved(on_button)], |ui| {
        ui.button("target");
        ui.ctx_mut().modal_push(menu_id(), menu);
    });

    h.frame(vec![button(on_button, MouseButton::Primary, true)], |ui| {
        clicked |= ui.button("target").pressed;
        // The menu's owner sees the dismissal and stops drawing it.
        assert!(ui.ctx().modal_dismissed(menu_id()).is_some());
    });
    assert!(
        !clicked,
        "the button under the menu must not take the press"
    );

    // Once the menu is gone, the very next press does reach the button.
    h.frame(vec![button(on_button, MouseButton::Primary, false)], |ui| {
        ui.button("target");
    });
    h.frame(vec![button(on_button, MouseButton::Primary, true)], |ui| {
        clicked |= ui.button("target").pressed;
    });
    assert!(clicked, "with no menu open the press lands normally");
}

#[test]
fn a_press_inside_the_menu_is_left_alone() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let inside = Pos2::new(240.0, 240.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(inside)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    ctx.begin_frame(input_at(
        0.1,
        vec![button(inside, MouseButton::Primary, true)],
    ));
    assert!(ctx.modal_dismissed(menu_id()).is_none(), "still open");
    assert!(
        ctx.input.pointer_pressed,
        "the menu's own rows still get their press"
    );
    ctx.end_frame();
}

#[test]
fn escape_pops_one_surface_and_is_consumed() {
    let outer = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 200.0));
    let inner = Rect::from_min_size(Pos2::new(280.0, 120.0), Vec2::new(160.0, 100.0));
    let inner_id = crusty_gui::id::Id::ROOT.with("submenu");

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![]));
    open_menu(&mut ctx, outer);
    ctx.modal_push(inner_id, inner);
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![key(Key::Escape, true)]));
    assert!(ctx.modal_dismissed(inner_id).is_some(), "the top one goes");
    assert!(ctx.modal_dismissed(menu_id()).is_none(), "its parent stays");
    assert!(
        !ctx.input.key_pressed(Key::Escape),
        "Escape is consumed, so it can't also cancel a drag underneath"
    );
    ctx.end_frame();
}

#[test]
fn a_press_in_a_child_surface_does_not_collapse_its_parent() {
    let parent = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 200.0));
    let child = Rect::from_min_size(Pos2::new(320.0, 120.0), Vec2::new(160.0, 100.0));
    let child_id = crusty_gui::id::Id::ROOT.with("submenu");
    let in_child = Pos2::new(360.0, 150.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(in_child)]));
    open_menu(&mut ctx, parent);
    ctx.modal_push(child_id, child);
    ctx.end_frame();

    ctx.begin_frame(input_at(
        0.1,
        vec![button(in_child, MouseButton::Primary, true)],
    ));
    assert!(ctx.modal_dismissed(menu_id()).is_none());
    assert!(ctx.modal_dismissed(child_id).is_none());
    assert!(
        ctx.input.pointer_pressed,
        "the child's rows get their press"
    );
    ctx.end_frame();
}

// ─────────────────────────────────────────────────────────────────────────────
// T3 — wheel outside closes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn t3_a_wheel_outside_an_open_menu_closes_it() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let outside = Pos2::new(50.0, 50.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(outside)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![Event::Scroll(Vec2::new(0.0, -3.0))]));
    assert_eq!(
        ctx.modal_dismissed(menu_id()),
        Some(crusty_gui::context::DismissCause::ScrollOutside)
    );
    assert_eq!(
        ctx.input.scroll_delta,
        Vec2::ZERO,
        "the dismissing wheel is consumed too — it should not also scroll the panel behind"
    );
    ctx.end_frame();
}

#[test]
fn a_wheel_inside_a_menu_scrolls_it_instead_of_closing() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let inside = Pos2::new(240.0, 240.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(inside)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![Event::Scroll(Vec2::new(0.0, -3.0))]));
    assert!(ctx.modal_dismissed(menu_id()).is_none());
    assert_ne!(ctx.input.scroll_delta, Vec2::ZERO);
    ctx.end_frame();
}

#[test]
fn losing_window_focus_dismisses_the_whole_stack_and_drops_held_input() {
    let outer = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 200.0));
    let inner = Rect::from_min_size(Pos2::new(280.0, 120.0), Vec2::new(160.0, 100.0));
    let inner_id = crusty_gui::id::Id::ROOT.with("submenu");

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(
        0.0,
        vec![button(Pos2::new(150.0, 150.0), MouseButton::Primary, true)],
    ));
    open_menu(&mut ctx, outer);
    ctx.modal_push(inner_id, inner);
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![Event::Focused(false)]));
    assert!(ctx.focus_lost());
    assert!(
        ctx.modal_dismissed(menu_id()).is_some(),
        "everything goes, not just the top"
    );
    assert!(ctx.modal_dismissed(inner_id).is_some());
    assert!(
        !ctx.input.pointer_down,
        "the OS took capture with it; a held button would otherwise stick forever"
    );
    ctx.end_frame();
}

#[test]
fn a_right_press_outside_hands_its_position_on_for_a_fresh_menu() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let elsewhere = Pos2::new(60.0, 400.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(elsewhere)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    ctx.begin_frame(input_at(
        0.1,
        vec![button(elsewhere, MouseButton::Secondary, true)],
    ));
    assert!(
        ctx.modal_dismissed(menu_id()).is_some(),
        "the old menu goes"
    );
    assert!(!ctx.input.right_pressed, "the press itself is consumed");
    assert_eq!(
        ctx.modal_reopen_at(),
        Some(elsewhere),
        "but its position is handed on, so one gesture both closes and reopens"
    );
    assert_eq!(ctx.take_modal_reopen(), Some(elsewhere));
    assert_eq!(ctx.modal_reopen_at(), None, "only one surface may claim it");
    ctx.end_frame();
}

// ─────────────────────────────────────────────────────────────────────────────
// T5 / T6 — the right button's dual use
// ─────────────────────────────────────────────────────────────────────────────

/// Drive `Canvas` for one frame and report `(right_clicked, view)`.
fn canvas_frame(h: &mut Harness, view: &mut CanvasView, events: Vec<Event>) -> Option<Pos2> {
    let mut out = None;
    h.frame(events, |ui| {
        out = Canvas::new().show(ui, view, |_, _| {}).right_clicked;
    });
    out
}

#[test]
fn t5_a_right_press_that_barely_moves_is_a_context_click() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let a = Pos2::new(200.0, 150.0);
    let b = Pos2::new(203.0, 150.0); // 3px — under the 4px threshold

    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(a)]);
    let before = view;
    canvas_frame(
        &mut h,
        &mut view,
        vec![button(a, MouseButton::Secondary, true)],
    );
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(b)]);
    let click = canvas_frame(
        &mut h,
        &mut view,
        vec![button(b, MouseButton::Secondary, false)],
    );

    assert_eq!(
        click,
        Some(a),
        "the menu opens at the press point, not the release"
    );
    assert_eq!(view, before, "and a context click never pans");
}

#[test]
fn t6_a_right_press_that_travels_pans_and_opens_no_menu() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let a = Pos2::new(200.0, 150.0);
    let b = Pos2::new(212.0, 150.0); // 12px — well past the threshold

    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(a)]);
    let before = view;
    canvas_frame(
        &mut h,
        &mut view,
        vec![button(a, MouseButton::Secondary, true)],
    );
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(b)]);
    let click = canvas_frame(
        &mut h,
        &mut view,
        vec![button(b, MouseButton::Secondary, false)],
    );

    assert_eq!(click, None, "a drag is a pan, so there is no context menu");
    assert_ne!(view.pan, before.pan, "and the canvas panned");
    // Dragging left moves the view right, i.e. `pan` decreases.
    assert!(view.pan.x < before.pan.x);
}

#[test]
fn middle_drag_still_pans_without_any_threshold() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let a = Pos2::new(200.0, 150.0);

    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(a)]);
    let before = view;
    canvas_frame(
        &mut h,
        &mut view,
        vec![button(a, MouseButton::Middle, true)],
    );
    canvas_frame(
        &mut h,
        &mut view,
        vec![Event::PointerMoved(Pos2::new(202.0, 150.0))],
    );
    assert_ne!(
        view.pan, before.pan,
        "middle has no other job, so it pans at once"
    );
}

#[test]
fn space_plus_left_drag_pans() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let a = Pos2::new(200.0, 150.0);

    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(a)]);
    let before = view;
    canvas_frame(&mut h, &mut view, vec![key(Key::Space, true)]);
    canvas_frame(
        &mut h,
        &mut view,
        vec![button(a, MouseButton::Primary, true)],
    );
    canvas_frame(
        &mut h,
        &mut view,
        vec![Event::PointerMoved(Pos2::new(220.0, 150.0))],
    );
    assert!(view.pan.x < before.pan.x, "Space+drag pans");

    // Releasing Space ends it; a plain left drag must not pan.
    canvas_frame(&mut h, &mut view, vec![key(Key::Space, false)]);
    let held = view;
    canvas_frame(
        &mut h,
        &mut view,
        vec![Event::PointerMoved(Pos2::new(260.0, 150.0))],
    );
    assert_eq!(view, held, "without Space the left button is the caller's");
}

/// Acceptance 11 — a touchpad flick must land a few ladder steps, not a
/// hundred micro-zooms.
#[test]
fn a_touchpad_flick_moves_a_few_ladder_steps_not_hundreds() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let p = Pos2::new(200.0, 150.0);
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(p)]);

    // Three tenths of a notch at a time — a plausible precision-touchpad
    // stream. Plain wheel, because that is zoom now.
    let flick = |h: &mut Harness, view: &mut CanvasView| {
        h.frame(vec![Event::Scroll(Vec2::new(0.0, 0.3))], |ui| {
            Canvas::new().show(ui, view, |_, _| {});
        });
    };
    for _ in 0..3 {
        flick(&mut h, &mut view);
    }
    assert_eq!(view.zoom, 1.0, "sub-notch dust must not zoom at all");

    flick(&mut h, &mut view);
    assert_eq!(
        view.zoom, 1.3501,
        "crossing one notch moves exactly one stop"
    );

    // Thirty more events — ten notches' worth — cannot outrun the ladder.
    for _ in 0..30 {
        flick(&mut h, &mut view);
    }
    assert_eq!(view.zoom, 2.20, "and it stops at the ladder's ceiling");
}

#[test]
fn a_plain_wheel_zooms_about_the_cursor_and_no_longer_pans() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let p = Pos2::new(300.0, 200.0);
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(p)]);

    // Ask the scope itself what is under the cursor, before and after — that
    // is the transform the body actually draws with.
    let mut world_before = Pos2::ZERO;
    h.frame(Vec::new(), |ui| {
        Canvas::new().show(ui, &mut view, |_, scope| {
            world_before = scope.screen_to_world(p);
        });
    });

    let mut world_after = Pos2::ZERO;
    h.frame(vec![Event::Scroll(Vec2::new(0.0, 1.0))], |ui| {
        Canvas::new().show(ui, &mut view, |_, scope| {
            world_after = scope.screen_to_world(p);
        });
    });

    assert_eq!(view.zoom, 1.3501, "a plain wheel steps the ladder");
    assert!(
        (world_after.x - world_before.x).abs() < 0.01
            && (world_after.y - world_before.y).abs() < 0.01,
        "and pins the point under the cursor ({world_before:?} -> {world_after:?})"
    );
}

#[test]
fn ctrl_wheel_leaves_the_ladder_and_zooms_past_the_ceiling() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView {
        pan: Vec2::ZERO,
        zoom: 2.20,
    };
    let p = Pos2::new(200.0, 150.0);
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(p)]);

    // Plain wheel is capped by the ladder...
    h.frame(vec![Event::Scroll(Vec2::new(0.0, 1.0))], |ui| {
        Canvas::new()
            .zoom_range(0.15, 4.0)
            .show(ui, &mut view, |_, _| {});
    });
    assert_eq!(view.zoom, 2.20, "the ladder's ceiling holds a plain wheel");

    // ...Ctrl is not.
    h.frame(vec![Event::Scroll(Vec2::new(0.0, 1.0))], |ui| {
        ui.ctx_mut().input.modifiers = Modifiers::CTRL;
        Canvas::new()
            .zoom_range(0.15, 4.0)
            .show(ui, &mut view, |_, _| {});
    });
    assert!(view.zoom > 2.20, "Ctrl goes past 1:1 for close work");
    assert!(view.zoom <= 4.0, "but still stops at the caller's limit");
}

#[test]
fn shift_wheel_pans_horizontally_rather_than_zooming() {
    let mut h = harness_or_skip!([400, 300]);
    let mut view = CanvasView::default();
    let p = Pos2::new(200.0, 150.0);
    canvas_frame(&mut h, &mut view, vec![Event::PointerMoved(p)]);
    let before = view;

    h.frame(vec![Event::Scroll(Vec2::new(0.0, 2.0))], |ui| {
        // Shift is carried on `RawInput.modifiers`, which the harness fixes
        // to empty; drive the modifier through the context directly.
        ui.ctx_mut().input.modifiers = Modifiers::SHIFT;
        Canvas::new().show(ui, &mut view, |_, _| {});
    });
    assert_eq!(view.zoom, before.zoom, "Shift+wheel never zooms");
    assert_ne!(view.pan.x, before.pan.x, "it pans horizontally");
    assert_eq!(view.pan.y, before.pan.y);
}

// ─────────────────────────────────────────────────────────────────────────────
// Key-repeat coalescing hook (the arrow-nudge gesture itself lands in Pass C)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_held_key_is_distinguishable_from_its_first_press() {
    let mut ctx = Context::new();
    let repeat = |pressed: bool, repeat: bool| Event::Key {
        key: Key::ArrowRight,
        pressed,
        repeat,
        modifiers: Modifiers::empty(),
    };

    ctx.begin_frame(input_at(0.0, vec![repeat(true, false)]));
    assert!(
        ctx.input.key_pressed_initial(Key::ArrowRight),
        "opens the transaction"
    );
    assert!(!ctx.input.key_repeated(Key::ArrowRight));
    assert!(ctx.input.key_down(Key::ArrowRight));
    ctx.end_frame();

    ctx.begin_frame(input_at(0.1, vec![repeat(true, true)]));
    assert!(
        !ctx.input.key_pressed_initial(Key::ArrowRight),
        "extends, never reopens"
    );
    assert!(ctx.input.key_repeated(Key::ArrowRight));
    assert!(
        ctx.input.key_pressed(Key::ArrowRight),
        "the coarse query still sees it"
    );
    ctx.end_frame();

    ctx.begin_frame(input_at(0.2, vec![repeat(false, false)]));
    assert!(
        !ctx.input.key_down(Key::ArrowRight),
        "released: commit the transaction"
    );
    ctx.end_frame();
}

// ─────────────────────────────────────────────────────────────────────────────
// Acceptance 2 / 4 — dismissal paths, end to end
// ─────────────────────────────────────────────────────────────────────────────

/// Acceptance 2 — right-pressing elsewhere closes the open menu *and* the same
/// gesture opens a new one where it landed.
#[test]
fn acceptance_2_a_right_press_elsewhere_closes_the_old_menu_and_opens_a_new_one() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));
    let elsewhere = Pos2::new(60.0, 400.0);

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(0.0, vec![Event::PointerMoved(elsewhere)]));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    ctx.begin_frame(input_at(
        0.1,
        vec![button(elsewhere, MouseButton::Secondary, true)],
    ));
    // Old one goes...
    assert!(
        ctx.modal_dismissed(menu_id()).is_some(),
        "the old menu closes"
    );
    // ...the press is swallowed so nothing underneath acts on it...
    assert!(
        !ctx.input.right_pressed,
        "the press does not reach the canvas"
    );
    // ...and its position is handed on, which is what makes it one gesture
    // rather than "close" then "open".
    let reopen = ctx.take_modal_reopen();
    assert_eq!(reopen, Some(elsewhere));
    // The owner opens a fresh surface there in the same frame.
    let fresh = Rect::from_min_size(elsewhere, Vec2::new(180.0, 120.0));
    ctx.modal_push(menu_id(), fresh);
    assert!(
        ctx.modal_is_open(menu_id()),
        "a new menu is open at the new point"
    );
    ctx.end_frame();

    // And it survives into the next frame rather than flickering shut.
    ctx.begin_frame(input_at(0.2, vec![]));
    assert!(ctx.modal_dismissed(menu_id()).is_none());
    ctx.end_frame();
}

/// Acceptance 4 — Alt-Tabbing away closes the menu. The window-manager route
/// reaches us only as `Focused(false)`, so that is what the test drives.
#[test]
fn acceptance_4_alt_tabbing_away_closes_the_menu() {
    let menu = Rect::from_min_size(Pos2::new(200.0, 200.0), Vec2::new(180.0, 120.0));

    let mut ctx = Context::new();
    ctx.begin_frame(input_at(
        0.0,
        vec![Event::PointerMoved(Pos2::new(240.0, 240.0))],
    ));
    open_menu(&mut ctx, menu);
    ctx.end_frame();

    // Alt goes down, then focus leaves — exactly the order winit delivers.
    ctx.begin_frame(input_at(
        0.1,
        vec![key(Key::Tab, true), Event::Focused(false)],
    ));
    assert!(ctx.focus_lost());
    assert_eq!(
        ctx.modal_dismissed(menu_id()),
        Some(crusty_gui::context::DismissCause::FocusLost),
        "the menu must not still be sitting there when the user comes back"
    );
    ctx.end_frame();

    // Coming back does not resurrect it.
    ctx.begin_frame(input_at(0.2, vec![Event::Focused(true)]));
    assert!(!ctx.focus_lost());
    assert!(!ctx.modal_any_open());
    ctx.end_frame();
}

/// Acceptance 12 — holding Alt must not activate a menu bar. We draw our own
/// menus, so there is nothing for the OS to activate; this pins that no code
/// path treats a bare Alt as an activation.
#[test]
fn acceptance_12_a_bare_alt_does_nothing_at_all() {
    let mut ctx = Context::new();
    let mut raw = input_at(0.0, vec![]);
    raw.modifiers = Modifiers::ALT;
    ctx.begin_frame(raw);
    assert!(
        ctx.input.key_presses.is_empty(),
        "no key press is synthesised"
    );
    assert!(!ctx.input.pointer_pressed);
    assert!(!ctx.modal_any_open(), "and nothing opens");
    let out = ctx.end_frame();
    assert!(
        !out.wants_keyboard,
        "a held modifier does not claim the keyboard"
    );
}
