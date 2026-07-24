use crate::context::Ui;
use crate::math::{Color, Pos2, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// A row that visually indicates whether `*value == option`. Clicking
/// sets `*value = option` and (if used inside a popup like
/// [`ComboBox`](crate::widgets::ComboBox)) signals the popup to close
/// via `Memory::popup_close_requested`.
pub struct SelectableValue<'a, T: PartialEq + Copy> {
    value: &'a mut T,
    option: T,
    label: String,
}

impl<'a, T: PartialEq + Copy> SelectableValue<'a, T> {
    pub fn new(value: &'a mut T, option: T, label: impl Into<String>) -> Self {
        Self {
            value,
            option,
            label: label.into(),
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(&self.label);

        let label_size = ui.text_mut().measure(&self.label, style.fonts.body, None);
        let height = label_size.y + 6.0;
        let rect = ui.allocate(Vec2::new(ui.available().width(), height));

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
        let mut clicked = false;
        if pointer_released && is_active && hovered {
            clicked = true;
            *self.value = self.option;
            // Ask any enclosing popup to close itself.
            ui.ctx_mut().memory.popup_close_requested = true;
        }

        let is_selected = *self.value == self.option;
        let fill = if is_selected {
            let mut accent = style.palette.accent_active;
            accent.a = 0.85;
            accent
        } else if hovered {
            style.palette.hover
        } else {
            Color::TRANSPARENT
        };

        if fill.a > 0.0 {
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                rect,
                style.rounding.small,
                fill,
            )));
        }

        let text_color = if is_selected {
            // Dark text on the bright accent fill for contrast.
            Color::rgba(0.10, 0.10, 0.12, 0.95)
        } else {
            style.palette.text
        };
        let text_pos = Pos2::new(
            rect.min.x + 8.0,
            rect.min.y + (rect.height() - label_size.y) * 0.5,
        );
        let Ui { ctx, text, .. } = ui;
        text.add_text(
            &mut ctx.paint,
            &self.label,
            style.fonts.body,
            text_pos,
            text_color,
            None,
        );

        Response {
            id,
            rect,
            hovered,
            pressed,
            clicked,
            hover_anim: if hovered { 1.0 } else { 0.0 },
            press_anim: if pressed { 1.0 } else { 0.0 },
        }
    }
}
