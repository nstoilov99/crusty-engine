//! Layout unit tests: widget rect placement (vertical stacking, horizontal
//! rows, spacing) asserted through real `Ui` frames on the shared headless
//! harness. Rects come from widget `Response`s, so these exercise the same
//! layout code paths the renderer sees — without comparing pixels.

#![cfg(feature = "vulkano-renderer")]

#[macro_use]
mod common;

use std::cell::Cell;

use crusty_gui::context::Ui;
use crusty_gui::math::Rect;

#[test]
fn vertical_stacking_with_uniform_spacing() {
    let mut h = harness_or_skip!([300, 200]);
    let rects: [Cell<Rect>; 3] = Default::default();
    h.frame(Vec::new(), |ui| {
        rects[0].set(ui.button("First").rect);
        rects[1].set(ui.button("Second").rect);
        rects[2].set(ui.button("Third").rect);
    });
    let [a, b, c] = rects.map(Cell::into_inner);

    assert_eq!(a.min.x, b.min.x, "top-down layout left-aligns widgets");
    assert_eq!(b.min.x, c.min.x);
    let gap_ab = b.min.y - a.max.y;
    let gap_bc = c.min.y - b.max.y;
    assert!(gap_ab > 0.0, "widgets don't touch (gap {gap_ab})");
    assert!(
        (gap_ab - gap_bc).abs() < 0.01,
        "item spacing is uniform ({gap_ab} vs {gap_bc})"
    );
}

#[test]
fn add_space_widens_the_gap_exactly() {
    let mut h = harness_or_skip!([300, 200]);
    let plain = Cell::new(0.0f32);
    let spaced = Cell::new(0.0f32);
    h.frame(Vec::new(), |ui| {
        let a = ui.button("A").rect;
        let b = ui.button("B").rect;
        plain.set(b.min.y - a.max.y);
        ui.add_space(24.0);
        let c = ui.button("C").rect;
        spaced.set(c.min.y - b.max.y);
    });
    assert!(
        (spaced.get() - (plain.get() + 24.0)).abs() < 0.01,
        "add_space(24) adds exactly 24 on top of item spacing \
         (plain {}, spaced {})",
        plain.get(),
        spaced.get()
    );
}

#[test]
fn horizontal_row_places_widgets_left_to_right() {
    let mut h = harness_or_skip!([400, 100]);
    let left = Cell::new(Rect::ZERO);
    let right = Cell::new(Rect::ZERO);
    h.frame(Vec::new(), |ui| {
        ui.horizontal(|ui| {
            left.set(ui.button("Left").rect);
            right.set(ui.button("Right").rect);
        });
    });
    let (l, r) = (left.get(), right.get());
    assert!(
        r.min.x >= l.max.x,
        "second widget sits to the right ({} vs {})",
        r.min.x,
        l.max.x
    );
    let v_overlap = l.max.y.min(r.max.y) - l.min.y.max(r.min.y);
    assert!(v_overlap > 0.0, "row widgets overlap vertically");
}

#[test]
fn label_width_grows_with_text() {
    let mut h = harness_or_skip!([400, 100]);
    let short = Cell::new(Rect::ZERO);
    let long = Cell::new(Rect::ZERO);
    h.frame(Vec::new(), |ui| {
        short.set(ui.label("hi").rect);
        long.set(ui.label("a much longer label with more glyphs").rect);
    });
    assert!(short.get().width() > 0.0);
    assert!(short.get().height() > 0.0);
    assert!(
        long.get().width() > short.get().width() * 3.0,
        "longer text produces a wider rect ({} vs {})",
        long.get().width(),
        short.get().width()
    );
}

#[test]
fn layout_is_stable_across_frames() {
    let mut h = harness_or_skip!([300, 200]);
    let mut checked = false;
    let mut slider = 0.5f32;
    let rects = Cell::new([Rect::ZERO; 3]);
    let mut build = |ui: &mut Ui| {
        let a = ui.button("Button").rect;
        let b = ui.checkbox(&mut checked, "Check").rect;
        let c = ui.slider(&mut slider, 0.0..=1.0).rect;
        rects.set([a, b, c]);
    };
    h.frame(Vec::new(), &mut build);
    let first = rects.get();
    for _ in 0..5 {
        h.frame(Vec::new(), &mut build);
    }
    let last = rects.get();
    assert_eq!(first, last, "rects must not drift across identical frames");
}
