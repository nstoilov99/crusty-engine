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
}

impl CollapsingHeader {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            default_open: false,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
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

        let chevron_size = style.fonts.body;
        let gap = style.spacing.box_label_gap;
        let label_size = ui.text_mut().measure(&self.label, style.fonts.body, None);
        let row_h = label_size.y.max(chevron_size) + 4.0;
        let header_rect =
            ui.allocate(Vec2::new(ui.available().width(), row_h));

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
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.press_anim =
            approach_half_life(mem.press_anim, target_press, dt, style.motion.press_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;
        let is_open = mem.bool_state;

        // Hover highlight strip — INSTANT: rows in property panels feel
        // sluggish with any easing on the tint, so we drive it from the
        // raw `hovered` bool. The animated `hover_anim` is still
        // computed for any caller that wants it via `Response`.
        let _ = hover_anim;
        if hovered {
            let tint = style.palette.surface_hover;
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                header_rect,
                style.rounding.small,
                tint,
            )));
        }

        // Chevron: switch between right-pointing triangle (closed) and
        // down-pointing triangle (open). Driven by the raw bool — no
        // animation lag.
        let chev_char = if is_open { '\u{25BC}' } else { '\u{25B6}' };
        let chev_str = chev_char.to_string();
        let chev_text_size =
            ui.text_mut().measure(&chev_str, style.fonts.body, None);
        let chev_x = header_rect.min.x + (chevron_size - chev_text_size.x) * 0.5;
        let chev_y = header_rect.min.y + (row_h - chev_text_size.y) * 0.5;
        // Chevron color flips instantly with hover too.
        let chev_color = if hovered {
            style.palette.accent
        } else {
            style.palette.text_dim
        };
        {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &chev_str,
                style.fonts.body,
                Pos2::new(chev_x, chev_y),
                chev_color,
                None,
            );
        }

        let label_x = header_rect.min.x + chevron_size + gap;
        let label_y = header_rect.min.y + (row_h - label_size.y) * 0.5;
        let label_color = style.palette.text;
        {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &self.label,
                style.fonts.body,
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
            let indent = (chevron_size + gap) * 1.6;
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

