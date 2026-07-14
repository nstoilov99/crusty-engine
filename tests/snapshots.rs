//! GPU snapshot tests: build a UI headlessly, render it through
//! [`TargetRenderer`](crusty_gui::backend::TargetRenderer) into an offscreen
//! image, read the pixels back and compare against golden PNGs in
//! `tests/snapshots/`.
//!
//! - `UPDATE_SNAPSHOTS=1 cargo test --test snapshots` regenerates goldens.
//! - On mismatch, `<name>.actual.png` and `<name>.diff.png` are written
//!   next to the golden for eyeballing.
//!
//! Harness and determinism notes live in `tests/common/mod.rs`.

#![cfg(feature = "vulkano-renderer")]

mod common;

use common::{click, release, Harness};
use crusty_gui::context::Ui;
use crusty_gui::input::Event;
use crusty_gui::math::{Pos2, Rect, Vec2};

#[test]
fn widgets_basic() {
    let mut h = Harness::new([420, 340]);
    let mut checked = true;
    let mut unchecked = false;
    let mut slider = 0.3f32;
    let paint = h.run_frames(3, |ui| {
        ui.label("crusty-gui snapshot: basic widgets");
        let _ = ui.button("Button");
        ui.checkbox(&mut checked, "Checked");
        ui.checkbox(&mut unchecked, "Unchecked");
        ui.slider(&mut slider, 0.0..=1.0);
        ui.progress_bar(0.6);
    });
    h.snapshot("widgets_basic", &paint);
    // Render the identical paint again: the renderer's hash-skip path must
    // reuse the cached mesh/buffers and still produce identical pixels.
    h.snapshot("widgets_basic", &paint);
}

#[test]
fn button_hover_and_press() {
    let mut h = Harness::new([260, 120]);
    let hover_rect = std::cell::Cell::new(Rect::ZERO);
    let press_rect = std::cell::Cell::new(Rect::ZERO);
    let build = |ui: &mut Ui| {
        hover_rect.set(ui.button("Hover me").rect);
        press_rect.set(ui.button("Press me").rect);
    };
    // Discovery frame to learn the button rects.
    h.frame(Vec::new(), build);
    let hover_pos = hover_rect.get().center();
    let press_pos = press_rect.get().center();

    h.frame(vec![Event::PointerMoved(hover_pos)], build);
    // Hold a press on the second button; extra frames settle hover animations.
    let mut paint = h.frame(click(press_pos), build);
    for _ in 0..30 {
        paint = h.frame(Vec::new(), build);
    }
    h.snapshot("button_hover_press", &paint);
}

#[test]
fn floating_window() {
    use crusty_gui::widgets::Window as GuiWindow;
    let mut h = Harness::new([420, 340]);
    let mut open = true;
    let mut slider = 0.7f32;
    let paint = h.run_frames(10, |ui| {
        ui.label("root content behind the window");
        ui.label("second root line");
        GuiWindow::new("Snapshot window")
            .open(&mut open)
            .default_pos(Pos2::new(90.0, 70.0))
            .default_size(Vec2::new(240.0, 160.0))
            .show(ui, |ui| {
                ui.label("window body");
                ui.slider(&mut slider, 0.0..=1.0);
                let _ = ui.button("A button");
            });
    });
    h.snapshot("floating_window", &paint);
}

#[test]
fn text_edit_focused() {
    let mut h = Harness::new([300, 90]);
    let mut value = String::from("hello");
    let mut rect = Rect::ZERO;
    h.frame(Vec::new(), |ui| {
        rect = ui.text_edit(&mut value).rect;
    });
    let pos = rect.center();
    // Click to focus, release, then type.
    h.frame(click(pos), |ui| {
        rect = ui.text_edit(&mut value).rect;
    });
    h.frame(release(pos), |ui| {
        rect = ui.text_edit(&mut value).rect;
    });
    h.frame(vec![Event::Text(" world".into())], |ui| {
        rect = ui.text_edit(&mut value).rect;
    });
    let paint = h.frame(Vec::new(), |ui| {
        rect = ui.text_edit(&mut value).rect;
    });
    assert_eq!(value, "hello world");
    h.snapshot("text_edit_focused", &paint);
}

#[test]
fn combo_popup_open() {
    use crusty_gui::widgets::ComboBox;
    let mut h = Harness::new([260, 220]);
    let mut quality = 1usize;
    let rect = std::cell::Cell::new(Rect::ZERO);
    let mut build = |ui: &mut Ui| {
        ui.label("Render quality");
        let labels = ["Low", "Medium", "High"];
        let resp = ComboBox::new("quality")
            .selected_text(labels[quality])
            .show_ui(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    ui.selectable_value(&mut quality, i, *label);
                }
            });
        rect.set(resp.rect);
    };
    h.frame(Vec::new(), &mut build);
    let pos = rect.get().center();
    // Click + release on the trigger opens the dropdown as a floating layer.
    h.frame(click(pos), &mut build);
    h.frame(release(pos), &mut build);
    let paint = h.run_frames(15, &mut build);
    h.snapshot("combo_popup_open", &paint);
}

#[test]
fn modal_window_dims_backdrop() {
    use crusty_gui::widgets::Window as GuiWindow;
    let mut h = Harness::new([420, 300]);
    let mut open = true;
    let paint = h.run_frames(10, |ui| {
        ui.label("root content that should be dimmed");
        let _ = ui.button("Unreachable button");
        GuiWindow::new("Modal")
            .open(&mut open)
            .modal(true)
            .resizable(false)
            .collapsible(false)
            .default_pos(Pos2::new(120.0, 90.0))
            .default_size(Vec2::new(200.0, 120.0))
            .show(ui, |ui| {
                ui.label("modal body");
                let _ = ui.button("OK");
            });
    });
    h.snapshot("modal_window", &paint);
}

#[test]
fn collapsing_open_and_closed() {
    use crusty_gui::widgets::CollapsingHeader;
    let mut h = Harness::new([300, 220]);
    let mut a = true;
    let mut b = false;
    let paint = h.run_frames(15, |ui| {
        CollapsingHeader::new("Open section")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(&mut a, "Inside open");
                ui.label("body line");
            });
        CollapsingHeader::new("Closed section").show(ui, |ui| {
            ui.checkbox(&mut b, "Hidden");
        });
    });
    h.snapshot("collapsing_sections", &paint);
}
