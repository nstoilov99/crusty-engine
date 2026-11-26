//! The two rounded-rect fill modes, proved at the pixel.
//!
//! `rect_filled` is the design system's *surface* fill: its alpha is a glass
//! tint strength over a blurred backdrop, and the interior is always fully
//! opaque. That is right for panels and popovers and wrong for anything that
//! must reveal live content — a selection marquee at "8% alpha" was rendering
//! as a solid slab that hid the nodes under it.
//!
//! `rect_filled_translucent` composites with real alpha. These tests are the
//! regression: they render one over the other and read the framebuffer back.

#[macro_use]
mod common;

use crusty_gui::math::{Color, Pos2, Rect, Rounding, Vec2};
use crusty_gui::paint::{PaintCmd, RectShape};

const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// An opaque red field with `overlay` drawn on top of its middle.
fn over_red(overlay: PaintCmd) -> Vec<PaintCmd> {
    let field = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0));
    vec![
        PaintCmd::Rrect(RectShape::fill(field, Rounding::ZERO, RED)),
        overlay,
    ]
}

fn patch() -> Rect {
    Rect::from_min_size(Pos2::new(50.0, 50.0), Vec2::new(100.0, 100.0))
}

#[test]
fn a_translucent_fill_lets_the_content_under_it_through() {
    let mut h = harness_or_skip!([200, 200]);
    let paint = over_red(PaintCmd::Rrect(RectShape::fill_translucent(
        patch(),
        Rounding::ZERO,
        BLUE.with_alpha(0.08),
    )));
    let buf = h.render_pixels(&paint);
    let [r, _g, b, _a] = h.pixel(&buf, 100, 100);

    assert!(
        r > 200,
        "8% blue over red must stay overwhelmingly red, got r={r}"
    );
    assert!(b > 0 && b < 60, "with a faint blue cast, got b={b}");
}

#[test]
fn the_glass_fill_is_opaque_by_construction() {
    // Not a bug — this is what panels and popovers want. The test pins the
    // distinction so nobody "fixes" glass into alpha and vice versa.
    let mut h = harness_or_skip!([200, 200]);
    let paint = over_red(PaintCmd::Rrect(RectShape::fill(
        patch(),
        Rounding::ZERO,
        BLUE.with_alpha(0.08),
    )));
    let buf = h.render_pixels(&paint);
    let [r, _g, _b, _a] = h.pixel(&buf, 100, 100);

    assert!(
        r < 200,
        "glass mode composites the blurred backdrop and hides what is under \
         it; if this ever passes through as red, the modes have been conflated"
    );
}

#[test]
fn a_translucent_fill_still_antialiases_its_rounded_corners() {
    let mut h = harness_or_skip!([200, 200]);
    let round = Rect::from_min_size(Pos2::new(50.0, 50.0), Vec2::new(100.0, 100.0));
    let paint = vec![PaintCmd::Rrect(RectShape::fill_translucent(
        round,
        Rounding::same(20.0),
        BLUE,
    ))];
    let buf = h.render_pixels(&paint);
    // The harness clears to a dark slate, so "untouched" is that, not zero.
    let cleared = h.pixel(&buf, 5, 5)[2];
    let outside = h.pixel(&buf, 30, 30)[2];
    let corner = h.pixel(&buf, 52, 52)[2];
    let edge = h.pixel(&buf, 52, 100)[2];
    let middle = h.pixel(&buf, 100, 100)[2];

    assert_eq!(outside, cleared, "clear of the shape entirely");
    assert!(middle > 200, "inside the shape");
    assert!(edge > 200, "the same inset along a straight edge is solid");
    assert!(
        corner < edge / 2,
        "the corner is carved away by the rounding (corner={corner}, edge={edge})"
    );
}

#[test]
fn opacity_scales_the_way_the_number_says() {
    let mut h = harness_or_skip!([200, 200]);
    let mut last = 255u8;
    for alpha in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let paint = over_red(PaintCmd::Rrect(RectShape::fill_translucent(
            patch(),
            Rounding::ZERO,
            BLUE.with_alpha(alpha),
        )));
        let buf = h.render_pixels(&paint);
        let r = h.pixel(&buf, 100, 100)[0];
        assert!(
            r <= last,
            "red must fall monotonically as blue's opacity rises \
             (alpha={alpha}, r={r}, previous={last})"
        );
        last = r;
    }
    assert!(
        last < 20,
        "at alpha 1.0 the red is fully covered, got {last}"
    );
}
