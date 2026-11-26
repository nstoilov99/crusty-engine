use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::widgets::button::lerp_color;

/// Toggle switch — 28×15 track with an 11px knob. Checked = accent track
/// + white knob; unchecked = neutral chrome.
pub struct Toggle<'a> {
    value: &'a mut bool,
    label: String,
    enabled: bool,
}

impl<'a> Toggle<'a> {
    pub fn new(value: &'a mut bool, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(("toggle", &self.label));
        let track_size = Vec2::new(28.0, 15.0);
        let gap = style.spacing.box_label_gap;
        let label_size = if self.label.is_empty() {
            Vec2::ZERO
        } else {
            ui.text_mut().measure(&self.label, style.fonts.body, None)
        };
        let total_w = track_size.x
            + if self.label.is_empty() {
                0.0
            } else {
                gap + label_size.x
            };
        let rect = ui.allocate(Vec2::new(total_w, track_size.y.max(label_size.y)));

        let hovered = self.enabled && ui.contains_pointer(rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let pointer_down = ui.ctx().input.pointer_down;
        let active_id = ui.ctx().memory.active_widget;
        if pointer_pressed && hovered {
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let is_active = self.enabled && (active_id == Some(id) || (pointer_pressed && hovered));
        let pressed = is_active && pointer_down;
        let mut clicked = false;
        if pointer_released && is_active && hovered {
            clicked = true;
        }
        let (focused, activated) = crate::widgets::focus_interact(ui, id, clicked);
        if self.enabled && activated {
            clicked = true;
        }
        if clicked {
            *self.value = !*self.value;
        }

        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        let target_on = if *self.value { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim = approach_half_life(
            mem.hover_anim,
            target_hover,
            dt,
            style.motion.hover_half_life,
        );
        mem.press_anim = approach_half_life(
            mem.press_anim,
            target_press,
            dt,
            style.motion.press_half_life,
        );
        mem.aux_anim =
            approach_half_life(mem.aux_anim, target_on, dt, style.motion.state_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;
        let on_anim = mem.aux_anim;

        let track_top = rect.min.y + (rect.height() - track_size.y) * 0.5;
        let track = Rect::from_min_size(Pos2::new(rect.min.x, track_top), track_size);

        let p = &style.palette;
        let (track_fill, track_stroke, knob_off, knob_on) = if !self.enabled {
            (
                p.header,
                p.stroke.with_alpha(p.stroke.a * 0.5),
                p.stroke_strong,
                p.stroke_strong,
            )
        } else {
            let stroke = if focused {
                p.focus_ring
            } else {
                lerp_color(
                    lerp_color(p.stroke, p.stroke_strong, hover_anim),
                    p.accent_active,
                    on_anim,
                )
            };
            (
                lerp_color(p.hover, p.accent_active, on_anim),
                stroke,
                p.text_secondary,
                crate::math::Color::WHITE,
            )
        };
        let shape = RectShape::fill(track, Rounding::same(track_size.y * 0.5), track_fill)
            .with_stroke(style.metrics.border, track_stroke);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        let knob_d = 11.0;
        let knob_x = track.min.x + 2.0 + (track_size.x - 4.0 - knob_d) * on_anim;
        let knob = Rect::from_min_size(
            Pos2::new(knob_x, track.min.y + (track_size.y - knob_d) * 0.5),
            Vec2::splat(knob_d),
        );
        let knob_fill = lerp_color(knob_off, knob_on, on_anim);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
            knob,
            Rounding::same(knob_d * 0.5),
            knob_fill,
        )));

        if !self.label.is_empty() {
            let label_color = if self.enabled {
                p.text
            } else {
                p.text_disabled
            };
            let text_pos = Pos2::new(
                track.max.x + gap,
                rect.min.y + (rect.height() - label_size.y) * 0.5,
            );
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &self.label,
                style.fonts.body,
                text_pos,
                label_color,
                None,
            );
        }

        Response {
            id,
            rect,
            hovered,
            pressed,
            clicked,
            hover_anim,
            press_anim,
        }
    }
}
