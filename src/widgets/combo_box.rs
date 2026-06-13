use crate::context::{Direction, Ui, UiOptions};
use crate::input::Key;
use crate::layer::Order;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// Dropdown selector. A trigger button that, when clicked, reveals a
/// popup containing a closure-provided list of options — typically
/// [`SelectableValue`](crate::widgets::SelectableValue)s.
///
/// The dropdown renders into [`Context::overlay_paint`] so it can float
/// above any UI rendered later in the same frame.
pub struct ComboBox {
    id_source: String,
    selected_text: String,
    width: Option<f32>,
}

impl ComboBox {
    pub fn new(id_source: impl Into<String>) -> Self {
        Self {
            id_source: id_source.into(),
            selected_text: String::new(),
            width: None,
        }
    }

    pub fn selected_text(mut self, text: impl Into<String>) -> Self {
        self.selected_text = text.into();
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn show_ui<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(("combo", &self.id_source));
        let width = self
            .width
            .unwrap_or_else(|| ui.available().width().min(220.0));
        let trigger_h = style.fonts.body + 12.0;
        let trigger_rect = ui.allocate(Vec2::new(width, trigger_h));

        let hovered = ui.contains_pointer(trigger_rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_pos = ui.ctx().input.pointer_pos;

        let was_open = ui.ctx().memory.get(id).map(|m| m.bool_state).unwrap_or(false);
        let mut is_open = was_open;

        if pointer_pressed && hovered {
            is_open = !is_open;
        }

        // ── Trigger render
        let fill = if hovered {
            style.palette.surface_hover
        } else {
            style.palette.surface
        };
        let stroke = if is_open {
            style.palette.accent
        } else {
            style.palette.stroke
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(trigger_rect, style.rounding.widget, fill)
                .with_stroke(1.0, stroke),
        ));

        // Selected text + ▼ arrow
        let arrow_str = "\u{25BC}".to_string();
        let arrow_size = ui.text_mut().measure(&arrow_str, style.fonts.body, None);
        let selected_pos = Pos2::new(
            trigger_rect.min.x + 8.0,
            trigger_rect.min.y + (trigger_h - style.fonts.body) * 0.5,
        );
        let arrow_pos = Pos2::new(
            trigger_rect.max.x - 8.0 - arrow_size.x,
            trigger_rect.min.y + (trigger_h - arrow_size.y) * 0.5,
        );
        {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &self.selected_text,
                style.fonts.body,
                selected_pos,
                style.palette.text,
                None,
            );
            text.add_text(
                &mut ctx.paint,
                &arrow_str,
                style.fonts.body,
                arrow_pos,
                style.palette.text_dim,
                None,
            );
        }

        // ── Dropdown render (overlay)
        let mut dropdown_rect = Rect::ZERO;
        if is_open {
            let dropdown_root = id.with("popup");
            let dropdown_x = trigger_rect.min.x;
            let dropdown_y = trigger_rect.max.y + 2.0;
            // Reserve a generous max height; the dropdown will be tighter
            // than this after we measure the body.
            let scratch_rect = Rect::from_min_size(
                Pos2::new(dropdown_x, dropdown_y),
                Vec2::new(width, 600.0),
            );

            // We need the body's commands to land *between* the dropdown
            // background and any later overlay commands, so they:
            //   1. sit above any UI rendered later in this frame, and
            //   2. sit above the dropdown's own background fill.
            // Easiest: push to ctx.paint while the body runs, then move
            // those commands into overlay_paint after sandwiching a
            // background underneath.
            let body_start_main = ui.ctx().paint.len();

            let popup_area_id = id.with("popup_area");
            let used_h = {
                let Ui { ctx, text, .. } = ui;
                let opts = UiOptions {
                    padding: Vec2::new(4.0, 4.0),
                    spacing: 2.0,
                };
                let mut inner = Ui::with_root(
                    ctx,
                    text,
                    scratch_rect,
                    Direction::TopDown,
                    opts,
                    dropdown_root,
                );
                inner.set_area(Some(popup_area_id));
                body(&mut inner);
                inner.used_rect().height()
            };

            let drained: Vec<PaintCmd> =
                ui.ctx_mut().paint.drain(body_start_main..).collect();

            dropdown_rect = Rect::from_min_size(
                Pos2::new(dropdown_x, dropdown_y),
                Vec2::new(width, (used_h + 8.0).max(trigger_h)),
            );

            // Now push background + drained body into overlay.
            ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
                RectShape::fill(
                    dropdown_rect,
                    style.rounding.widget,
                    style.palette.surface_hover,
                )
                .with_stroke(1.0, style.palette.stroke)
                .with_shadow(20.0, Color::rgba(0.0, 0.0, 0.0, 0.45)),
            ));
            ui.ctx_mut().overlay_paint.extend(drained);

            // Register the dropdown as a top-order interaction area so
            // its items win pointer priority over any window beneath.
            let area = ui.ctx_mut().memory.area_entry(
                popup_area_id,
                dropdown_rect.min,
                Order::Tooltip,
            );
            area.pos = dropdown_rect.min;
            area.size = dropdown_rect.size();

            // ── Close conditions
            if ui.ctx().memory.popup_close_requested {
                is_open = false;
            }
            if ui.ctx().input.key_pressed(Key::Escape) {
                is_open = false;
            }
            // Click outside trigger AND dropdown closes.
            if pointer_pressed {
                if let Some(p) = pointer_pos {
                    if !trigger_rect.contains(p) && !dropdown_rect.contains(p) {
                        is_open = false;
                    }
                }
            }
        }

        // ── Persist state
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = trigger_rect;
        mem.bool_state = is_open;

        Response {
            id,
            rect: trigger_rect,
            hovered,
            pressed: false,
            clicked: pointer_pressed && hovered,
            hover_anim: if hovered { 1.0 } else { 0.0 },
            press_anim: 0.0,
        }
    }
}
