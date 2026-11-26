use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::context::Ui;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::text::FontFamily;

/// Hover delay before a tooltip appears.
const DELAY: f64 = 0.4;

/// Structured tooltip: title, optional secondary meta line ("Mesh · 75.2 KB")
/// and optional mono path line. Single instance app-wide — when several
/// hovered widgets request one in the same frame, the last request (the
/// topmost widget) wins; it appears after the pointer rests 400 ms on the
/// same anchor, cursor-anchored and clamped to the screen.
pub struct Tooltip {
    title: String,
    meta: Option<String>,
    path: Option<String>,
}

impl Tooltip {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            meta: None,
            path: None,
        }
    }

    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Request this tooltip while the pointer hovers `rect`.
    pub fn show_for(self, ui: &mut Ui, rect: Rect) {
        if !ui.contains_pointer(rect) || ui.ctx().input.pointer_down {
            return;
        }
        let Some(pointer) = ui.ctx().input.pointer_pos else {
            return;
        };
        let anchor = anchor_hash(rect);
        ui.ctx_mut().tooltip.pending = Some(anchor);

        let committed = ui.ctx().tooltip.committed;
        let elapsed = ui.ctx().time() - ui.ctx().tooltip.hover_start;
        if committed != Some(anchor) {
            // Not yet the winner — arbitration lands next frame.
            ui.ctx_mut()
                .request_repaint_after(Duration::from_millis(16));
            return;
        }
        if elapsed < DELAY {
            ui.ctx_mut()
                .request_repaint_after(Duration::from_secs_f64(DELAY - elapsed));
            return;
        }
        if ui.ctx().tooltip.shown_this_frame {
            return;
        }
        ui.ctx_mut().tooltip.shown_this_frame = true;
        self.paint(ui, pointer);
    }

    fn paint(self, ui: &mut Ui, pointer: Pos2) {
        let style = ui.style();
        let title_font = style.fonts.body;
        let sub_font = style.fonts.small;
        let line_gap = 3.0;
        let pad = Vec2::new(10.0, 8.0);

        let title_size = ui.text_mut().measure(&self.title, title_font, None);
        let meta_size = self
            .meta
            .as_ref()
            .map(|m| ui.text_mut().measure(m, sub_font, None));
        let path_size = self.path.as_ref().map(|p| {
            ui.text_mut()
                .measure_family(p, sub_font, None, FontFamily::Mono)
        });

        let mut content = title_size;
        for extra in [meta_size, path_size].into_iter().flatten() {
            content.x = content.x.max(extra.x);
            content.y += line_gap + extra.y;
        }
        let tip_size = content + pad * 2.0;

        // Cursor-anchored; slide left at the right edge, flip above the
        // pointer at the bottom edge.
        let screen = ui.ctx().screen_rect();
        let margin = 4.0;
        let x = (pointer.x + 12.0)
            .min(screen.max.x - tip_size.x - margin)
            .max(screen.min.x + margin);
        let mut y = pointer.y + 18.0;
        if y + tip_size.y > screen.max.y - margin {
            y = pointer.y - 8.0 - tip_size.y;
        }
        let y = y.max(screen.min.y + margin);
        let tip_rect = Rect::from_min_size(Pos2::new(x, y), tip_size);

        ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
            RectShape::fill(
                tip_rect,
                style.rounding.small,
                style
                    .palette
                    .elevated
                    .with_alpha(style.palette.popover_alpha),
            )
            .with_stroke(style.metrics.border, style.palette.stroke_strong),
        ));

        let mut cursor = tip_rect.min + pad;
        let Ui { ctx, text, .. } = ui;
        text.add_text(
            &mut ctx.overlay_paint,
            &self.title,
            title_font,
            cursor,
            style.palette.text,
            None,
        );
        cursor.y += title_size.y + line_gap;
        if let Some(meta) = &self.meta {
            text.add_text(
                &mut ctx.overlay_paint,
                meta,
                sub_font,
                cursor,
                style.palette.text_secondary,
                None,
            );
            cursor.y += meta_size.expect("measured above").y + line_gap;
        }
        if let Some(path) = &self.path {
            text.add_text_family(
                &mut ctx.overlay_paint,
                path,
                sub_font,
                cursor,
                style.palette.text_mono,
                None,
                FontFamily::Mono,
            );
        }
    }
}

/// Plain single-line tooltip while the pointer hovers `rect`.
pub fn show_tooltip_for(ui: &mut Ui, rect: Rect, text: &str) {
    Tooltip::new(text).show_for(ui, rect);
}

fn anchor_hash(rect: Rect) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rect.min.x.to_bits().hash(&mut h);
    rect.min.y.to_bits().hash(&mut h);
    rect.max.x.to_bits().hash(&mut h);
    rect.max.y.to_bits().hash(&mut h);
    h.finish()
}
