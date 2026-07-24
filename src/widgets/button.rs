use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// Visual role per the design system's button family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Neutral chrome: `header` fill, `stroke` border.
    #[default]
    Standard,
    /// Accent-filled call to action (Save All).
    Primary,
    /// Filled destructive action (Remove) — confirm dialogs.
    Danger,
    /// No fill/border at rest; neutral hover fill (Cancel, menu-ish).
    Ghost,
}

pub struct Button {
    text: String,
    size: Option<f32>,
    min_size: Option<Vec2>,
    exact_size: Option<Vec2>,
    fill: Option<Color>,
    text_color: Option<Color>,
    variant: ButtonVariant,
    toggled: bool,
    enabled: bool,
}

impl Button {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: None,
            min_size: None,
            exact_size: None,
            fill: None,
            text_color: None,
            variant: ButtonVariant::Standard,
            toggled: false,
            enabled: true,
        }
    }

    pub fn size(mut self, size_px: f32) -> Self {
        self.size = Some(size_px);
        self
    }

    /// Lower bound on the button rect (text + padding still wins when
    /// larger). Useful for square icon-ish buttons with narrow labels.
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Pin the button rect to this size regardless of text metrics
    /// (like egui's `add_sized`). The label stays centered.
    pub fn exact_size(mut self, size: Vec2) -> Self {
        self.exact_size = Some(size);
        self
    }

    /// Override the background fill (hover/press lighten it slightly).
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Override the label color.
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    /// Toggled-tool state: `accent_soft` fill + accent border + accent
    /// label — the only accent-filled thing in a toolbar.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = on;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let font_size = self.size.unwrap_or(style.fonts.body);
        let id = ui.alloc_id(&self.text);

        let text_size = ui.text_mut().measure(&self.text, font_size, None);
        let min = self.min_size.unwrap_or(Vec2::ZERO);
        let button_size = self.exact_size.unwrap_or(Vec2::new(
            (text_size.x + style.spacing.button_padding.x * 2.0).max(min.x),
            (text_size.y + style.spacing.button_padding.y * 2.0).max(min.y),
        ));
        let rect = ui.allocate(button_size);

        let hovered = self.enabled && ui.contains_pointer(rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let pointer_down = ui.ctx().input.pointer_down;
        let active_id = ui.ctx().memory.active_widget;
        let mut clicked = false;
        if pointer_pressed && hovered {
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let is_active = self.enabled && (active_id == Some(id) || (pointer_pressed && hovered));
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

        // State ladder: hover = fill one step lighter + stroke_strong;
        // pressed = one step darker; disabled = header fill, 50% stroke,
        // disabled text. Flat — no glow, no border widening.
        let p = &style.palette;
        struct Colors {
            base: Color,
            hover: Color,
            press: Color,
            border: Color,
            border_hover: Color,
            text: Color,
        }
        let c = if !self.enabled {
            Colors {
                base: p.header,
                hover: p.header,
                press: p.header,
                border: p.stroke.with_alpha(p.stroke.a * 0.5),
                border_hover: p.stroke.with_alpha(p.stroke.a * 0.5),
                text: p.text_disabled,
            }
        } else if self.toggled {
            Colors {
                base: p.accent_soft,
                hover: p.accent_soft,
                press: p.accent_soft,
                border: p.accent_active,
                border_hover: p.accent_active,
                text: p.accent_active,
            }
        } else {
            match self.variant {
                ButtonVariant::Standard => Colors {
                    base: self.fill.unwrap_or(p.header),
                    hover: self.fill.map(|f| lighten(f, 0.05)).unwrap_or(p.hover),
                    press: self.fill.map(|f| darken(f, 0.04)).unwrap_or(p.panel),
                    border: p.stroke,
                    border_hover: p.stroke_strong,
                    text: p.text,
                },
                ButtonVariant::Primary => Colors {
                    base: p.accent_active,
                    hover: lighten(p.accent_active, 0.09),
                    press: darken(p.accent_active, 0.07),
                    border: p.accent_active,
                    border_hover: lighten(p.accent_active, 0.09),
                    text: p.accent_text,
                },
                ButtonVariant::Danger => Colors {
                    base: p.danger,
                    hover: lighten(p.danger, 0.07),
                    press: darken(p.danger, 0.07),
                    border: p.danger,
                    border_hover: lighten(p.danger, 0.07),
                    text: Color::WHITE,
                },
                ButtonVariant::Ghost => Colors {
                    base: Color::TRANSPARENT,
                    hover: p.hover,
                    press: darken(p.hover, 0.04),
                    border: Color::TRANSPARENT,
                    border_hover: Color::TRANSPARENT,
                    text: p.text_secondary,
                },
            }
        };

        let mut fill = lerp_color(c.base, c.hover, hover_anim);
        fill = lerp_color(fill, c.press, press_anim);
        let stroke = lerp_color(c.border, c.border_hover, hover_anim);
        let shape = RectShape::fill(rect, style.rounding.widget, fill)
            .with_stroke(style.metrics.border, stroke);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        let text_color = self.text_color.unwrap_or(if self.enabled
            && self.variant == ButtonVariant::Ghost
            && !self.toggled
        {
            lerp_color(c.text, p.text, hover_anim)
        } else {
            c.text
        });
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
            text_color,
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

pub(crate) fn lighten(c: Color, amt: f32) -> Color {
    Color::rgba(
        (c.r + amt).min(1.0),
        (c.g + amt).min(1.0),
        (c.b + amt).min(1.0),
        c.a,
    )
}

pub(crate) fn darken(c: Color, amt: f32) -> Color {
    Color::rgba(
        (c.r - amt).max(0.0),
        (c.g - amt).max(0.0),
        (c.b - amt).max(0.0),
        c.a,
    )
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub(crate) fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        lerp(a.r, b.r, t),
        lerp(a.g, b.g, t),
        lerp(a.b, b.b, t),
        lerp(a.a, b.a, t),
    )
}
