//! Headless tests for the Phase 9 layer / area system: z-ordering,
//! pointer occlusion, modal blocking and layered paint composition.

use crusty_gui::context::Context;
use crusty_gui::id::Id;
use crusty_gui::input::{Event, RawInput};
use crusty_gui::layer::Order;
use crusty_gui::math::{Color, Pos2, Rect, Vec2};
use crusty_gui::paint::PaintCmd;

fn raw_input(pointer: Pos2) -> RawInput {
    RawInput {
        screen_rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
        pixels_per_point: 1.0,
        predicted_dt: 1.0 / 60.0,
        events: vec![Event::PointerMoved(pointer)],
        ..Default::default()
    }
}

fn register_area(ctx: &mut Context, id: Id, pos: Pos2, size: Vec2, order: Order) {
    let area = ctx.memory.area_entry(id, pos, order);
    area.pos = pos;
    area.size = size;
}

#[test]
fn topmost_area_wins_pointer() {
    let mut ctx = Context::new();
    let a = Id::new("a");
    let b = Id::new("b");
    // Overlapping areas; b registered later → higher z.
    register_area(&mut ctx, a, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);
    register_area(&mut ctx, b, Pos2::new(50.0, 50.0), Vec2::new(100.0, 100.0), Order::Middle);

    ctx.begin_frame(raw_input(Pos2::new(75.0, 75.0)));
    assert_eq!(ctx.hovered_area(), Some(b));
    // Root widgets under the overlap must not react.
    assert!(!ctx.pointer_over(None, Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))));
    // Widgets of the occluded area must not react either.
    assert!(!ctx.pointer_over(Some(a), Rect::from_min_size(Pos2::new(70.0, 70.0), Vec2::new(10.0, 10.0))));
    // Widgets of the top area do.
    assert!(ctx.pointer_over(Some(b), Rect::from_min_size(Pos2::new(70.0, 70.0), Vec2::new(10.0, 10.0))));
    register_area(&mut ctx, a, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);
    register_area(&mut ctx, b, Pos2::new(50.0, 50.0), Vec2::new(100.0, 100.0), Order::Middle);
    ctx.end_frame();

    // Bring a to the front → it now owns the overlap.
    ctx.memory.bring_area_to_front(a);
    assert!(ctx.memory.is_area_front(a));
    ctx.begin_frame(raw_input(Pos2::new(75.0, 75.0)));
    assert_eq!(ctx.hovered_area(), Some(a));
    ctx.end_frame();
}

#[test]
fn pointer_outside_areas_belongs_to_root() {
    let mut ctx = Context::new();
    let a = Id::new("a");
    register_area(&mut ctx, a, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);

    ctx.begin_frame(raw_input(Pos2::new(400.0, 400.0)));
    assert_eq!(ctx.hovered_area(), None);
    assert!(ctx.pointer_over(None, Rect::from_min_size(Pos2::new(390.0, 390.0), Vec2::new(20.0, 20.0))));
    ctx.end_frame();
}

#[test]
fn modal_blocks_everything_beneath() {
    let mut ctx = Context::new();
    let panel = Id::new("panel");
    let modal = Id::new("modal");
    register_area(&mut ctx, panel, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);
    register_area(&mut ctx, modal, Pos2::new(200.0, 200.0), Vec2::new(100.0, 100.0), Order::Modal);

    // Pointer over the non-modal area → still blocked by the modal.
    ctx.begin_frame(raw_input(Pos2::new(50.0, 50.0)));
    assert_eq!(ctx.hovered_area(), Some(modal));
    assert!(!ctx.pointer_over(Some(panel), Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(20.0, 20.0))));
    register_area(&mut ctx, panel, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);
    register_area(&mut ctx, modal, Pos2::new(200.0, 200.0), Vec2::new(100.0, 100.0), Order::Modal);
    ctx.end_frame();

    // Pointer over the modal itself → modal widgets interactable.
    ctx.begin_frame(raw_input(Pos2::new(250.0, 250.0)));
    assert_eq!(ctx.hovered_area(), Some(modal));
    assert!(ctx.pointer_over(Some(modal), Rect::from_min_size(Pos2::new(240.0, 240.0), Vec2::new(20.0, 20.0))));
    register_area(&mut ctx, panel, Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Order::Middle);
    ctx.end_frame();

    // Modal not shown this frame → next frame the panel is free again.
    ctx.begin_frame(raw_input(Pos2::new(50.0, 50.0)));
    assert_eq!(ctx.hovered_area(), Some(panel));
    ctx.end_frame();
}

#[test]
fn tooltip_order_beats_modal() {
    let mut ctx = Context::new();
    let modal = Id::new("modal");
    let popup = Id::new("popup");
    register_area(&mut ctx, modal, Pos2::new(0.0, 0.0), Vec2::new(300.0, 300.0), Order::Modal);
    register_area(&mut ctx, popup, Pos2::new(100.0, 100.0), Vec2::new(50.0, 50.0), Order::Tooltip);

    ctx.begin_frame(raw_input(Pos2::new(120.0, 120.0)));
    assert_eq!(ctx.hovered_area(), Some(popup));
    ctx.end_frame();
}

#[test]
fn layers_compose_in_z_order() {
    let mut ctx = Context::new();
    let back = Id::new("back");
    let front = Id::new("front");

    ctx.begin_frame(raw_input(Pos2::new(0.0, 0.0)));
    register_area(&mut ctx, back, Pos2::ZERO, Vec2::new(10.0, 10.0), Order::Middle);
    register_area(&mut ctx, front, Pos2::ZERO, Vec2::new(10.0, 10.0), Order::Middle);
    // `front` registered later → higher z, must render after `back`
    // even though we paint it first.
    let marker = |v: f32| PaintCmd::Rect {
        rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)),
        color: Color::rgba(v, 0.0, 0.0, 1.0),
    };

    ctx.paint.push(marker(0.1)); // root

    ctx.begin_layer();
    ctx.paint.push(marker(0.3));
    ctx.end_layer(front, Order::Middle);

    ctx.begin_layer();
    ctx.paint.push(marker(0.2));
    ctx.end_layer(back, Order::Middle);

    ctx.overlay_paint.push(marker(0.4)); // legacy overlay → very top

    let output = ctx.end_frame();
    let reds: Vec<f32> = output
        .paint
        .iter()
        .map(|cmd| match cmd {
            PaintCmd::Rect { color, .. } => color.r,
            _ => panic!("unexpected cmd"),
        })
        .collect();
    assert_eq!(reds, vec![0.1, 0.2, 0.3, 0.4]);
}
