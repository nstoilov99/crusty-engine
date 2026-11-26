use crate::context::Ui;
use crate::math::{Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::widgets::button::lighten;

/// Count chip — a pill filter carrying a status color and a count
/// (console "Errors 2"). Active = status tint fill (13%) + status border +
/// colored dot; muted = neutral outline + gray dot. The count stays
/// visible either way.
pub struct CountChip {
    label: String,
    color: crate::math::Color,
    active: bool,
}

impl CountChip {
    pub fn new(label: impl Into<String>, color: crate::math::Color) -> Self {
        Self {
            label: label.into(),
            color,
            active: true,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let font = style.fonts.small;
        let height = 20.0;
        let dot = 6.0;
        let pad = 8.0;
        let gap = 6.0;
        let id = ui.alloc_id(("count_chip", &self.label));
        let label_size = ui.text_mut().measure(&self.label, font, None);
        let rect = ui.allocate(Vec2::new(
            pad + dot + gap + label_size.x + pad,
            height,
        ));
        let (response, focused) = chip_interact(ui, id, rect);

        let p = &style.palette;
        let (fill, mut border, dot_color, text_color) = if self.active {
            (
                self.color.with_alpha(0.13),
                self.color.with_alpha(0.5),
                self.color,
                lighten(self.color, 0.12),
            )
        } else {
            (
                crate::math::Color::TRANSPARENT,
                p.stroke,
                p.stroke_strong,
                p.text_disabled,
            )
        };
        if focused {
            border = p.focus_ring;
        }
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, Rounding::same(height * 0.5), fill)
                .with_stroke(style.metrics.border, border),
        ));
        ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
            Rect::from_center_size(
                Pos2::new(rect.min.x + pad + dot * 0.5, rect.center().y),
                Vec2::splat(dot),
            ),
            Rounding::same(dot * 0.5),
            dot_color,
        )));
        let text_pos = Pos2::new(
            rect.min.x + pad + dot + gap,
            rect.min.y + (height - label_size.y) * 0.5,
        );
        let Ui { ctx, text, .. } = ui;
        text.add_text(&mut ctx.paint, &self.label, font, text_pos, text_color, None);
        response
    }
}

/// Filter chip — neutral toggle (asset browser "All / Meshes"). Active =
/// `active` fill + primary text; rest = quiet outline. Square-ish
/// `small` rounding — pills are reserved for count chips.
pub struct FilterChip {
    label: String,
    active: bool,
}

impl FilterChip {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            active: false,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let font = style.fonts.small;
        let height = 20.0;
        let pad = 8.0;
        let id = ui.alloc_id(("filter_chip", &self.label));
        let label_size = ui.text_mut().measure(&self.label, font, None);
        let rect = ui.allocate(Vec2::new(pad + label_size.x + pad, height));
        let (response, focused) = chip_interact(ui, id, rect);

        let p = &style.palette;
        let (fill, mut border, text_color) = if self.active {
            (p.active, p.stroke_strong, p.text)
        } else if response.hovered {
            (p.hover, p.stroke, p.text_secondary)
        } else {
            (crate::math::Color::TRANSPARENT, p.stroke, p.text_secondary)
        };
        if focused {
            border = p.focus_ring;
        }
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, style.rounding.small, fill)
                .with_stroke(style.metrics.border, border),
        ));
        let text_pos = Pos2::new(
            rect.min.x + pad,
            rect.min.y + (height - label_size.y) * 0.5,
        );
        let Ui { ctx, text, .. } = ui;
        text.add_text(&mut ctx.paint, &self.label, font, text_pos, text_color, None);
        response
    }
}

fn chip_interact(ui: &mut Ui, id: crate::id::Id, rect: Rect) -> (Response, bool) {
    let hovered = ui.contains_pointer(rect);
    let pointer_pressed = ui.ctx().input.pointer_pressed;
    let pointer_released = ui.ctx().input.pointer_released;
    let pointer_down = ui.ctx().input.pointer_down;
    let active_id = ui.ctx().memory.active_widget;
    if pointer_pressed && hovered {
        ui.ctx_mut().memory.active_widget = Some(id);
    }
    let is_active = active_id == Some(id) || (pointer_pressed && hovered);
    let pressed = is_active && pointer_down;
    let mut clicked = pointer_released && is_active && hovered;
    let (focused, activated) = crate::widgets::focus_interact(ui, id, clicked);
    if activated {
        clicked = true;
    }
    let mem = ui.ctx_mut().memory.get_or_default(id);
    mem.seen_this_frame = true;
    mem.last_rect = rect;
    (
        Response {
            id,
            rect,
            hovered,
            pressed,
            clicked,
            hover_anim: if hovered { 1.0 } else { 0.0 },
            press_anim: if pressed { 1.0 } else { 0.0 },
        },
        focused,
    )
}
