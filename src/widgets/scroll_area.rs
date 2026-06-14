use crate::animation::approach_half_life;
use crate::context::{Direction, Ui, UiOptions};
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// A scrollable viewport. `max_height` caps the rect we allocate; if the
/// inner content is taller, the user can scroll with the wheel or by
/// dragging the scrollbar.
pub struct ScrollArea {
    max_height: f32,
    auto_shrink: bool,
}

impl ScrollArea {
    pub fn new(max_height: f32) -> Self {
        Self {
            max_height,
            auto_shrink: true,
        }
    }

    /// If true (default), the area shrinks to its content's height when
    /// the content is shorter than `max_height`. Disable for fixed-size
    /// scrolling regions.
    pub fn auto_shrink(mut self, v: bool) -> Self {
        self.auto_shrink = v;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
        let style = ui.style();
        let id = ui.alloc_id("scroll_area");
        let width = ui.available().width();

        // Snapshot last-frame content height + scroll offset.
        let (prev_offset_y, prev_content_h) = match ui.ctx().memory.get(id) {
            Some(m) => (m.aux_vec2.y, m.aux_anim),
            None => (0.0, 0.0),
        };

        // Decide the outer rect height. If auto-shrink and the content
        // fits, shrink. Otherwise lock to max_height.
        let outer_h = if self.auto_shrink && prev_content_h > 0.0 {
            prev_content_h.min(self.max_height)
        } else {
            self.max_height
        };
        let outer_rect = ui.allocate(Vec2::new(width, outer_h));

        let scrollbar_w = 8.0;
        let inner_width = (outer_rect.width() - scrollbar_w - 4.0).max(1.0);
        // Inner Ui starts at a y shifted up by the scroll offset so
        // content "slides" under the clip rect.
        let inner_top = outer_rect.min.y - prev_offset_y;
        let inner_rect = Rect::from_min_size(
            Pos2::new(outer_rect.min.x, inner_top),
            Vec2::new(inner_width, prev_content_h.max(outer_h)),
        );

        // Clip everything we draw inside to the outer rect.
        ui.ctx_mut().paint.push(PaintCmd::PushClip(outer_rect));

        let inner_root = id.with("inner");
        let (result, used_h) = {
            // A small horizontal inset so widgets flush against the start of
            // the row (a left-edge button's hover glow, a stroke) aren't
            // clipped by the viewport's left edge. Vertical padding stays 0
            // so content still aligns with the scroll offset.
            let inner_opts = UiOptions {
                padding: Vec2::new(8.0, 0.0),
                spacing: style.spacing.item,
            };
            let parent_area = ui.area();
            let Ui { ctx, text, .. } = ui;
            let mut inner = Ui::with_root(
                ctx,
                text,
                inner_rect,
                Direction::TopDown,
                inner_opts,
                inner_root,
            );
            inner.set_area(parent_area);
            let result = body(&mut inner);
            let used = (inner.used_rect().height()).max(0.0);
            (result, used)
        };

        ui.ctx_mut().paint.push(PaintCmd::PopClip);

        // Handle mouse wheel.
        let pointer_in_outer = ui.contains_pointer(outer_rect);
        let max_offset = (used_h - outer_rect.height()).max(0.0);
        let mut new_offset = prev_offset_y.clamp(0.0, max_offset);
        if pointer_in_outer {
            let wheel = ui.ctx().input.scroll_delta.y;
            if wheel != 0.0 {
                new_offset = (new_offset - wheel * 30.0).clamp(0.0, max_offset);
            }
        }

        // Render scrollbar if there's overflow.
        if max_offset > 0.5 {
            let track_rect = Rect::from_min_size(
                Pos2::new(outer_rect.max.x - scrollbar_w, outer_rect.min.y),
                Vec2::new(scrollbar_w, outer_rect.height()),
            );
            let mut track_color = style.palette.surface_active;
            track_color.a = 0.6;
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                track_rect,
                Rounding::same(scrollbar_w * 0.5),
                track_color,
            )));

            let thumb_ratio = (outer_rect.height() / used_h).clamp(0.05, 1.0);
            let thumb_h = (outer_rect.height() * thumb_ratio).max(20.0);
            let thumb_track = outer_rect.height() - thumb_h;
            let progress = if max_offset > 0.0 {
                new_offset / max_offset
            } else {
                0.0
            };
            let thumb_y = outer_rect.min.y + thumb_track * progress;
            let thumb_rect = Rect::from_min_size(
                Pos2::new(outer_rect.max.x - scrollbar_w + 1.0, thumb_y),
                Vec2::new(scrollbar_w - 2.0, thumb_h),
            );

            // Thumb hover + drag.
            let thumb_id = id.with("thumb");
            let thumb_hovered = ui.contains_pointer(thumb_rect);
            let pointer_pressed = ui.ctx().input.pointer_pressed;
            let pointer_down = ui.ctx().input.pointer_down;
            let active_id = ui.ctx().memory.active_widget;
            if pointer_pressed && thumb_hovered {
                ui.ctx_mut().memory.active_widget = Some(thumb_id);
            }
            let is_active =
                active_id == Some(thumb_id) || (pointer_pressed && thumb_hovered);
            if is_active && pointer_down {
                if let Some(p) = ui.ctx().input.pointer_pos {
                    let track_top = outer_rect.min.y + thumb_h * 0.5;
                    let track_bot = outer_rect.max.y - thumb_h * 0.5;
                    let t = ((p.y - track_top) / (track_bot - track_top).max(1e-3))
                        .clamp(0.0, 1.0);
                    new_offset = t * max_offset;
                }
            }

            let dt = ui.ctx().dt();
            let thumb_mem = ui.ctx_mut().memory.get_or_default(thumb_id);
            thumb_mem.seen_this_frame = true;
            thumb_mem.last_rect = thumb_rect;
            let hover_target = if thumb_hovered || is_active { 1.0 } else { 0.0 };
            thumb_mem.hover_anim = approach_half_life(
                thumb_mem.hover_anim,
                hover_target,
                dt,
                style.motion.hover_half_life,
            );
            let hover_anim = thumb_mem.hover_anim;

            let thumb_color = lerp_color(
                style.palette.text_dim,
                style.palette.accent,
                hover_anim,
            );
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                thumb_rect,
                Rounding::same(scrollbar_w * 0.5 - 1.0),
                thumb_color,
            )));
        }

        // Save state for next frame.
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = outer_rect;
        mem.aux_vec2.y = new_offset;
        mem.aux_anim = used_h;
        let _ = pointer_in_outer;

        result
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
