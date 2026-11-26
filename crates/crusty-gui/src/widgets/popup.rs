use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::layer::Order;
use crate::math::{Rect, Vec2};
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
        let popup_area_id = self.id.with("popup_area");
        let mut is_open = Self::is_open(ui, self.id);
        if ui.ctx().modal_dismissed(popup_area_id).is_some() {
            is_open = false;
        }
        if is_open {
            let style = ui.style();
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
            // Overlay commands a child emits meanwhile (e.g. a combo's
            // dropdown list) must land *above* the popup's own fill, so
            // they are drained too and re-appended last.
            let body_start = ui.ctx().paint.len();
            let overlay_start = ui.ctx().overlay_paint.len();
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
            let overlay_drained: Vec<PaintCmd> =
                ui.ctx_mut().overlay_paint.drain(overlay_start..).collect();

            let popup_rect = Rect::from_min_size(pos, Vec2::new(self.width, used_h + 8.0));
            ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
                RectShape::fill(
                    popup_rect,
                    style.rounding.panel,
                    style
                        .palette
                        .elevated
                        .with_alpha(style.palette.popover_alpha),
                )
                .with_stroke(style.metrics.border, style.palette.stroke_strong),
            ));
            ui.ctx_mut().overlay_paint.extend(drained);
            ui.ctx_mut().overlay_paint.extend(overlay_drained);

            // Top-order interaction area so items win pointer priority.
            let area =
                ui.ctx_mut()
                    .memory
                    .area_entry(popup_area_id, popup_rect.min, Order::Tooltip);
            area.pos = popup_rect.min;
            area.size = popup_rect.size();

            // Rule 1: dismissal is decided once, in `Context::begin_frame`.
            // The registered rect spans the anchor too, because the anchor's
            // own press is the caller's toggle and must reach it rather than
            // being swallowed as an outside-press. Nesting is handled by stack
            // order — a child popup pushed later shields this one — which
            // replaces the old `topmost_area_contains` special case.
            ui.ctx_mut()
                .modal_push(popup_area_id, self.anchor.union(popup_rect));

            if ui.ctx().memory.popup_close_requested {
                is_open = false;
            }
        }

        if !is_open {
            ui.ctx_mut().modal_dismiss(popup_area_id);
        }

        let mem = ui.ctx_mut().memory.get_or_default(self.id);
        mem.seen_this_frame = true;
        mem.bool_state = is_open;
    }
}
