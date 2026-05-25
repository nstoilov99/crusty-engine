use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::input::Key;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// Single-line text input.
///
/// Phase 7 basics: click to focus, type to append, Backspace removes the
/// last character, Esc / Enter / clicking elsewhere defocuses. Cursor is
/// always at the end of the string; selection, arrow keys, and clipboard
/// integration are left to a later TextEdit polish pass.
pub struct TextEdit<'a> {
    value: &'a mut String,
    width: Option<f32>,
    hint: Option<String>,
}

impl<'a> TextEdit<'a> {
    pub fn new(value: &'a mut String) -> Self {
        Self {
            value,
            width: None,
            hint: None,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Placeholder text shown (dimmed) when the value is empty.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id("text_edit");
        let width = self
            .width
            .unwrap_or_else(|| ui.available().width().min(220.0));
        let height = style.fonts.body + 10.0;
        let rect = ui.allocate(Vec2::new(width, height));

        // Hit-test for focus.
        let hovered = ui.ctx().input.pointer_in(rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        if pointer_pressed && hovered {
            // Claim focus; Context cleared `focused_widget` to None at
            // the start of this frame so the most-recently-clicked
            // widget wins.
            ui.ctx_mut().memory.focused_widget = Some(id);
        }
        let focused = ui.ctx().memory.focused_widget == Some(id);

        // Apply input if focused.
        let mut clicked = pointer_pressed && hovered;
        if focused {
            // Insert typed characters at end (Phase 7 keeps cursor at
            // end of string).
            let typed = std::mem::take(&mut ui.ctx_mut().input.typed_text);
            for ch in typed.chars() {
                if !ch.is_control() {
                    self.value.push(ch);
                }
            }

            // Handle special keys.
            let key_presses: Vec<_> = ui.ctx().input.key_presses.clone();
            for press in key_presses {
                match press.key {
                    Key::Backspace => {
                        self.value.pop();
                    }
                    Key::Escape | Key::Enter => {
                        ui.ctx_mut().memory.focused_widget = None;
                        clicked = true;
                    }
                    _ => {}
                }
            }
        }

        // Animation state.
        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_focus = if focused { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.aux_anim =
            approach_half_life(mem.aux_anim, target_focus, dt, style.motion.state_half_life);
        let hover_anim = mem.hover_anim;
        let focus_anim = mem.aux_anim;

        // Background + border. Stroke turns into accent color when
        // focused.
        let fill = style.palette.surface_active;
        let stroke = lerp_color(
            lerp_color(style.palette.stroke, style.palette.stroke_hover, hover_anim),
            style.palette.accent,
            focus_anim,
        );
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, style.rounding.widget, fill)
                .with_stroke(1.0 + focus_anim * 0.5, stroke),
        ));

        // Render text (or hint when empty + not focused).
        let text_pad = 6.0;
        let text_y = rect.min.y + (rect.height() - style.fonts.body) * 0.5;
        let text_pos = Pos2::new(rect.min.x + text_pad, text_y);

        let display_text;
        let display_color;
        if self.value.is_empty() && !focused {
            display_text = self.hint.as_deref().unwrap_or("");
            display_color = style.palette.text_dim;
        } else {
            display_text = self.value.as_str();
            display_color = style.palette.text;
        }
        let text_size = if display_text.is_empty() {
            Vec2::new(0.0, style.fonts.body)
        } else {
            ui.text_mut()
                .measure(display_text, style.fonts.body, None)
        };
        if !display_text.is_empty() {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                display_text,
                style.fonts.body,
                text_pos,
                display_color,
                None,
            );
        }

        // Caret. Blinks while focused (simple square-wave on `time`).
        if focused {
            let blink_on = ((ui.ctx().time() * 2.0).fract()) < 0.55;
            if blink_on {
                let caret_x = text_pos.x + text_size.x + 1.0;
                let caret_rect = Rect::from_min_size(
                    Pos2::new(caret_x, text_y - 1.0),
                    Vec2::new(1.5, style.fonts.body + 2.0),
                );
                ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                    caret_rect,
                    style.rounding.small,
                    style.palette.accent,
                )));
            }
        }

        Response {
            id,
            rect,
            hovered,
            pressed: focused,
            clicked,
            hover_anim,
            press_anim: focus_anim,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        lerp(a.r, b.r, t),
        lerp(a.g, b.g, t),
        lerp(a.b, b.b, t),
        lerp(a.a, b.a, t),
    )
}
