use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::input::Key;
use crate::layer::Order;
use crate::math::{Color, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// Anchored dropdown popup — the ComboBox overlay mechanism as a reusable
/// widget, for custom triggers (split buttons, toolbar menus).
///
/// Open state persists in [`Memory`](crate::context::memory::Memory) under
/// `id`. Call [`toggle`](Self::toggle) when the trigger is clicked, then
/// [`show`](Self::show) every frame; the body renders into the overlay
/// (above all same-frame UI) while open. Closes on Escape, click outside
/// the anchor and popup, or when a child requests it (e.g. a clicked
/// [`SelectableValue`](crate::widgets::SelectableValue) setting
/// `popup_close_requested`).
pub struct Popup {
    id: Id,
    anchor: Rect,
    width: f32,
}

impl Popup {
    pub fn new(id: Id, anchor: Rect, width: f32) -> Self {
        Self { id, anchor, width }
    }

    pub fn is_open(ui: &Ui, id: Id) -> bool {
        ui.ctx()
            .memory
            .get(id)
            .map(|m| m.bool_state)
            .unwrap_or(false)
    }

    pub fn toggle(ui: &mut Ui, id: Id) {
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.bool_state = !mem.bool_state;
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
        let mut is_open = Self::is_open(ui, self.id);
        if is_open {
            let style = ui.style();
            let popup_area_id = self.id.with("popup_area");
            let pos = super::popup_pos(
                ui,
                popup_area_id,
                self.anchor,
                self.width,
                self.anchor.height(),
                2.0,
            );
            let scratch = Rect::from_min_size(pos, Vec2::new(self.width, 600.0));

            // Body commands go to the main list first, then move to the
            // overlay above a background fill (same trick as ComboBox).
            let body_start = ui.ctx().paint.len();
            let used_h = {
                let Ui { ctx, text, .. } = ui;
                let opts = UiOptions {
                    padding: Vec2::new(4.0, 4.0),
                    spacing: 2.0,
                };
                let mut inner = Ui::with_root(
                    ctx,
                    text,
                    scratch,
                    Direction::TopDown,
                    opts,
                    self.id.with("popup"),
                );
                inner.set_area(Some(popup_area_id));
                body(&mut inner);
                inner.used_rect().height()
            };
            let drained: Vec<PaintCmd> = ui.ctx_mut().paint.drain(body_start..).collect();

            let popup_rect = Rect::from_min_size(pos, Vec2::new(self.width, used_h + 8.0));
            ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
                RectShape::fill(popup_rect, style.rounding.widget, style.palette.surface_hover)
                    .with_stroke(1.0, style.palette.stroke)
                    .with_shadow(20.0, Color::rgba(0.0, 0.0, 0.0, 0.45)),
            ));
            ui.ctx_mut().overlay_paint.extend(drained);

            // Top-order interaction area so items win pointer priority.
            let area = ui
                .ctx_mut()
                .memory
                .area_entry(popup_area_id, popup_rect.min, Order::Tooltip);
            area.pos = popup_rect.min;
            area.size = popup_rect.size();

            if ui.ctx().memory.popup_close_requested {
                is_open = false;
            }
            if ui.ctx().input.key_pressed(Key::Escape) {
                is_open = false;
            }
            // Click outside anchor AND popup closes (the anchor's own click
            // is the caller's toggle).
            if ui.ctx().input.pointer_pressed
                && ui
                    .ctx()
                    .input
                    .pointer_pos
                    .is_some_and(|p| !self.anchor.contains(p) && !popup_rect.contains(p))
            {
                is_open = false;
            }
        }

        let mem = ui.ctx_mut().memory.get_or_default(self.id);
        mem.seen_this_frame = true;
        mem.bool_state = is_open;
    }
}
