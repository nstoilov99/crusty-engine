use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::widgets::button::lerp_color;

pub struct Checkbox<'a> {
    value: &'a mut bool,
    label: String,
    mixed: bool,
    enabled: bool,
}

impl<'a> Checkbox<'a> {
    pub fn new(value: &'a mut bool, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            mixed: false,
            enabled: true,
        }
    }

    /// Mixed/indeterminate: accent box with a dash instead of a check
    /// (multi-selection with differing values). Clicking resolves to
    /// checked.
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(&self.label);
        let box_size = style.sizes.checkbox;
        let gap = style.spacing.box_label_gap;
        let label_size = ui.text_mut().measure(&self.label, style.fonts.body, None);
        let total = Vec2::new(box_size + gap + label_size.x, box_size.max(label_size.y));
        let rect = ui.allocate(total);

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
            // A mixed box resolves to checked on the first click.
            *self.value = if self.mixed { true } else { !*self.value };
        }

        let on = *self.value || self.mixed;
        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        let target_check = if on { 1.0 } else { 0.0 };
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
            approach_half_life(mem.aux_anim, target_check, dt, style.motion.state_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;
        let check_anim = mem.aux_anim;

        let box_top = rect.min.y + (rect.height() - box_size) * 0.5;
        let box_rect = Rect::from_min_size(Pos2::new(rect.min.x, box_top), Vec2::splat(box_size));

        let p = &style.palette;
        let (fill, outline, mark_color, label_color) = if !self.enabled {
            (
                p.header,
                p.stroke.with_alpha(p.stroke.a * 0.5),
                p.text_disabled,
                p.text_disabled,
            )
        } else {
            let off_outline = lerp_color(p.stroke_strong, p.text_disabled, hover_anim);
            let outline = if focused {
                p.focus_ring
            } else {
                lerp_color(off_outline, p.accent_active, check_anim)
            };
            (
                lerp_color(p.input, p.accent_active, check_anim),
                outline,
                p.accent_text,
                p.text,
            )
        };
        let shape = RectShape::fill(box_rect, style.rounding.small, fill)
            .with_stroke(style.metrics.border, outline);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        if check_anim > 0.01 {
            let mut mark_color = mark_color;
            mark_color.a *= check_anim;
            let mark = if self.mixed {
                // Dash: half box width, 2px tall, centered.
                Rect::from_center_size(
                    box_rect.center(),
                    Vec2::new(box_size * 0.5 * check_anim, 2.0),
                )
            } else {
                let inset = box_size * 0.5 * (1.0 - check_anim);
                Rect::from_min_size(
                    Pos2::new(box_rect.min.x + inset, box_rect.min.y + inset),
                    Vec2::splat(box_size - inset * 2.0),
                )
                .shrink(box_size * 0.22)
            };
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                mark,
                style.rounding.small,
                mark_color,
            )));
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
            label_color,
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
