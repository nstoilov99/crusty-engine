use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

pub struct Checkbox<'a> {
    value: &'a mut bool,
    label: String,
}

impl<'a> Checkbox<'a> {
    pub fn new(value: &'a mut bool, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(&self.label);
        let box_size = style.sizes.checkbox;
        let gap = style.spacing.box_label_gap;
        let label_size = ui.text_mut().measure(&self.label, style.fonts.body, None);
        let total = Vec2::new(box_size + gap + label_size.x, box_size.max(label_size.y));
        let rect = ui.allocate(total);

        let hovered = ui.ctx().input.pointer_in(rect);
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
            *self.value = !*self.value;
        }

        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        let target_check = if *self.value { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.press_anim =
            approach_half_life(mem.press_anim, target_press, dt, style.motion.press_half_life);
        mem.aux_anim =
            approach_half_life(mem.aux_anim, target_check, dt, style.motion.state_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;
        let check_anim = mem.aux_anim;

        let box_top = rect.min.y + (rect.height() - box_size) * 0.5;
        let box_rect =
            Rect::from_min_size(Pos2::new(rect.min.x, box_top), Vec2::splat(box_size));

        let fill = lerp_color(style.palette.surface, style.palette.accent, check_anim);
        let outline =
            lerp_color(style.palette.stroke, style.palette.stroke_hover, hover_anim);
        let shape = RectShape::fill(box_rect, style.rounding.small, fill)
            .with_stroke(1.0 + hover_anim * 0.4, outline);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        if check_anim > 0.01 {
            let inset = box_size * 0.5 * (1.0 - check_anim);
            let inner = Rect::from_min_size(
                Pos2::new(box_rect.min.x + inset, box_rect.min.y + inset),
                Vec2::splat(box_size - inset * 2.0),
            )
            .shrink(box_size * 0.18);
            let mark = RectShape::fill(
                inner,
                style.rounding.small,
                Color::rgba(1.0, 1.0, 1.0, 0.95 * check_anim),
            );
            ui.ctx_mut().paint.push(PaintCmd::Rrect(mark));
        }
        let _ = press_anim;

        let text_pos = Pos2::new(
            box_rect.max.x + gap,
            rect.min.y + (rect.height() - label_size.y) * 0.5,
        );
        let Ui { ctx, text, .. } = ui;
        text.add_text(
            &mut ctx.paint,
            &self.label,
            style.fonts.body,
            text_pos,
            style.palette.text,
            None,
        );

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
