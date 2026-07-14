//! Frame-lifecycle tests: `FrameOutput::paint_changed` must reflect whether
//! the merged paint list's content differs from the previous frame's.

use crusty_gui::context::Context;
use crusty_gui::input::{Modifiers, RawInput};
use crusty_gui::math::{Color, Pos2, Rect, Vec2};
use crusty_gui::paint::PaintCmd;

fn input_at(time: f64) -> RawInput {
    RawInput {
        screen_rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
        pixels_per_point: 1.0,
        time,
        predicted_dt: 1.0 / 60.0,
        modifiers: Modifiers::empty(),
        events: Vec::new(),
    }
}

fn rect_cmd(color: Color) -> PaintCmd {
    PaintCmd::Rect {
        rect: Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(50.0, 20.0)),
        color,
    }
}

#[test]
fn paint_changed_tracks_content_across_frames() {
    let mut ctx = Context::new();

    ctx.begin_frame(input_at(0.0));
    ctx.paint.push(rect_cmd(Color::rgba(1.0, 0.0, 0.0, 1.0)));
    assert!(ctx.end_frame().paint_changed, "first frame always changed");

    ctx.begin_frame(input_at(0.1));
    ctx.paint.push(rect_cmd(Color::rgba(1.0, 0.0, 0.0, 1.0)));
    assert!(
        !ctx.end_frame().paint_changed,
        "identical paint is not a change"
    );

    ctx.begin_frame(input_at(0.2));
    ctx.paint.push(rect_cmd(Color::rgba(0.0, 1.0, 0.0, 1.0)));
    assert!(ctx.end_frame().paint_changed, "color change is a change");

    ctx.begin_frame(input_at(0.3));
    assert!(ctx.end_frame().paint_changed, "emptying the paint is a change");

    ctx.begin_frame(input_at(0.4));
    assert!(!ctx.end_frame().paint_changed, "still empty — no change");
}
