use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Pos2, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// A clickable header that hides/shows its body. Persists open state
/// across frames in [`WidgetMemory::bool_state`](crate::context::WidgetMemory).
pub struct CollapsingHeader {
    label: String,
    default_open: bool,
    text_size: Option<f32>,
    text_color: Option<crate::math::Color>,
    fit_width: bool,
}

impl CollapsingHeader {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            default_open: false,
            text_size: None,
            text_color: None,
            fit_width: false,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Override the header label's font size (default: body). The body
    /// indent stays keyed to the body font so nesting depth is uniform.
    pub fn text_size(mut self, size: f32) -> Self {
        self.text_size = Some(size);
        self
    }

    pub fn text_color(mut self, color: crate::math::Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Allocate only the chevron + label width instead of the full
    /// available width, so enclosing content-fitted frames stay compact.
    pub fn fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }

    /// Shows the header. The closure runs only if the header is open
    /// (after click + animation start). Returns the header's response —
    /// the closure's return value is discarded for now; if you need it,
    /// stash it via mutable capture.
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(&self.label);

        // Initialise on first sight.
        let first_time = ui.ctx().memory.get(id).is_none();
        if first_time {
            let mem = ui.ctx_mut().memory.get_or_default(id);
            mem.bool_state = self.default_open;
            mem.aux_anim = if self.default_open { 1.0 } else { 0.0 };
        }

        let font = self.text_size.unwrap_or(style.fonts.body);
        let chevron_size = font;
        let gap = style.spacing.box_label_gap;
        let label_size = ui.text_mut().measure(&self.label, font, None);
        let row_h = label_size.y.max(chevron_size) + 4.0;
        let header_w = if self.fit_width {
            chevron_size + gap + label_size.x + 4.0
        } else {
            ui.available().width()
        };
        let header_rect = ui.allocate(Vec2::new(header_w, row_h));

        let hovered = ui.contains_pointer(header_rect);
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
            let mem = ui.ctx_mut().memory.get_or_default(id);
            mem.bool_state = !mem.bool_state;
        }

        // Phase 8 polish: open/close is INSTANT. Property panels feel
        // slow with any easing on the body. The animated hover_anim /
        // press_anim still get computed for the Response so callers can
        // animate themselves if they want.
        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_press = if pressed { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = header_rect;
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
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;
        let is_open = mem.bool_state;

        // Hover highlight strip — INSTANT: rows in property panels feel
        // sluggish with any easing on the tint, so we drive it from the
        // raw `hovered` bool. The animated `hover_anim` is still
        // computed for any caller that wants it via `Response`.
        let _ = hover_anim;
        if hovered {
            let tint = style.palette.hover;
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                header_rect,
                style.rounding.small,
                tint,
            )));
        }

        // Disclosure chevron: down "v" when open, right ">" when closed.
        // Driven by the raw bool — no animation lag.
        let chev_color = if hovered {
            style.palette.accent_active
        } else {
            style.palette.text_secondary
        };
        let center = Pos2::new(
            header_rect.min.x + chevron_size * 0.5,
            header_rect.min.y + row_h * 0.5,
        );
        crate::widgets::paint_chevron(
            &mut ui.ctx_mut().paint,
            center,
            font * 0.30,
            (font * 0.12).max(1.5),
            is_open,
            chev_color,
        );

        let label_x = header_rect.min.x + chevron_size + gap;
        let label_y = header_rect.min.y + (row_h - label_size.y) * 0.5;
        let label_color = self.text_color.unwrap_or(style.palette.text);
        {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &self.label,
                font,
                Pos2::new(label_x, label_y),
                label_color,
                None,
            );
        }
        let _ = press_anim;

        if is_open {
            // Indent the body so the hierarchy is clearly nested.
            // ~1.6× the chevron width — children sit visibly to the
            // right of the parent's label, matching Unity/Unreal-style
            // outliners.
            let indent = (style.fonts.body + gap) * 1.6;
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.vertical(|ui| {
                    body(ui);
                });
            });
        }

        Response {
            id,
            rect: header_rect,
            hovered,
            pressed,
            clicked,
            hover_anim,
            press_anim,
        }
    }
}
