use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

pub struct Button {
    text: String,
    size: Option<f32>,
    fill: Option<Color>,
    text_color: Option<Color>,
}

impl Button {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: None,
            fill: None,
            text_color: None,
        }
    }

    pub fn size(mut self, size_px: f32) -> Self {
        self.size = Some(size_px);
        self
    }

    /// Override the background fill (hover/press lighten it slightly).
    /// Used for toggle-style buttons that tint by state, like the console
    /// log filters.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Override the label color.
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let font_size = self.size.unwrap_or(style.fonts.body);
        let id = ui.alloc_id(&self.text);

        let text_size = ui.text_mut().measure(&self.text, font_size, None);
        let button_size = Vec2::new(
            text_size.x + style.spacing.button_padding.x * 2.0,
            text_size.y + style.spacing.button_padding.y * 2.0,
        );
        let rect = ui.allocate(button_size);

        let hovered = ui.contains_pointer(rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let pointer_down = ui.ctx().input.pointer_down;
        let active_id = ui.ctx().memory.active_widget;
        let mut clicked = false;
        if pointer_pressed && hovered {
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let is_active = active_id == Some(id) || (pointer_pressed && hovered);
        let pressed = is_active && pointer_down;
        if pointer_released && is_active && hovered {
            clicked = true;
        }

        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.press_anim =
            approach_half_life(mem.press_anim, target_press, dt, style.motion.press_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        // Compose fill: base → hover → active. A custom fill lightens by state
        // instead of using the palette surfaces.
        let (base, hover_c, active_c) = match self.fill {
            Some(c) => (c, lighten(c, 0.05), lighten(c, 0.10)),
            None => (
                style.palette.surface,
                style.palette.surface_hover,
                style.palette.surface_active,
            ),
        };
        let mut fill = lerp_color(base, hover_c, hover_anim);
        fill = lerp_color(fill, active_c, press_anim);

        let stroke =
            lerp_color(style.palette.stroke, style.palette.stroke_hover, hover_anim);
        let shape = RectShape::fill(rect, style.rounding.widget, fill)
            .with_stroke(1.0 + hover_anim * 0.5, stroke);
        let shape = if hover_anim > 0.0 {
            let mut glow = style.palette.accent_glow;
            glow.a *= hover_anim;
            shape.with_glow(8.0 + 4.0 * hover_anim, glow)
        } else {
            shape
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        let text_pos = Pos2::new(
            rect.min.x + (rect.width() - text_size.x) * 0.5,
            rect.min.y + (rect.height() - text_size.y) * 0.5 + press_anim * 1.0,
        );
        let Ui { ctx, text, .. } = ui;
        text.add_text(
            &mut ctx.paint,
            &self.text,
            font_size,
            text_pos,
            self.text_color.unwrap_or(style.palette.text),
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

fn lighten(c: Color, amt: f32) -> Color {
    Color::rgba(
        (c.r + amt).min(1.0),
        (c.g + amt).min(1.0),
        (c.b + amt).min(1.0),
        c.a,
    )
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
