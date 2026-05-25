use std::ops::RangeInclusive;

use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

pub struct Slider<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    width: Option<f32>,
}

impl<'a> Slider<'a> {
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self {
            value,
            range,
            width: None,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id((
            "slider",
            &self.range.start().to_bits(),
            &self.range.end().to_bits(),
        ));
        let width = self.width.unwrap_or(style.sizes.slider_default_width);
        let height = style.sizes.slider_height;
        let rect = ui.allocate(Vec2::new(width, height));

        let min = *self.range.start();
        let max = *self.range.end();
        let span = (max - min).max(1e-6);

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

        if pressed {
            if let Some(p) = ui.ctx().input.pointer_pos {
                let t = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                *self.value = min + t * span;
            }
        }
        let mut clicked = false;
        if pointer_released && is_active && hovered {
            clicked = true;
        }

        let value = self.value.clamp(min, max);
        let t = ((value - min) / span).clamp(0.0, 1.0);

        let dt = ui.ctx().dt();
        let target_hover = if hovered || pressed { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.press_anim =
            approach_half_life(mem.press_anim, target_press, dt, style.motion.press_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        let track_h = height * style.sizes.slider_track_ratio;
        let track_top = rect.min.y + (rect.height() - track_h) * 0.5;
        let track = Rect::from_min_size(
            Pos2::new(rect.min.x, track_top),
            Vec2::new(rect.width(), track_h),
        );
        let track_shape = RectShape::fill(
            track,
            Rounding::same(track_h * 0.5),
            style.palette.surface_active,
        );
        ui.ctx_mut().paint.push(PaintCmd::Rrect(track_shape));

        let filled =
            Rect::from_min_size(track.min, Vec2::new(track.width() * t, track_h));
        if filled.width() > 0.0 {
            let filled_shape = RectShape::fill(
                filled,
                Rounding::same(track_h * 0.5),
                style.palette.accent,
            );
            ui.ctx_mut().paint.push(PaintCmd::Rrect(filled_shape));
        }

        let handle_radius = height * 0.5 + hover_anim * 1.5;
        let handle_x = rect.min.x + rect.width() * t;
        let handle_center = Pos2::new(handle_x, rect.min.y + rect.height() * 0.5);
        let handle_rect =
            Rect::from_center_size(handle_center, Vec2::splat(handle_radius * 2.0));
        let handle_fill = lerp_color(style.palette.text, style.palette.text_dim, press_anim);
        let handle_shape = RectShape::fill(handle_rect, Rounding::same(handle_radius), handle_fill)
            .with_stroke(
                1.0,
                lerp_color(style.palette.stroke, style.palette.accent, hover_anim),
            );
        let handle_shape = if hover_anim > 0.0 {
            let mut glow = style.palette.accent_glow;
            glow.a *= hover_anim;
            handle_shape.with_glow(6.0 + 4.0 * hover_anim, glow)
        } else {
            handle_shape
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(handle_shape));

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
