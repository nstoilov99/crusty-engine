//! Transient toast notifications.
//!
//! Toasts are pushed from anywhere during a frame and live in a single
//! persistent queue in [`Memory`](crate::context::Memory)'s typed store.
//! Call [`show_toasts`](Ui::show_toasts) once per frame (after the rest of
//! the UI) to advance their ages, paint the stack in the top-right corner,
//! and drop expired ones. Each toast fades in briefly and fades out over its
//! final stretch.

use crate::context::Ui;
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// Severity of a toast — drives its accent stripe color.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Clone)]
struct Toast {
    text: String,
    kind: ToastKind,
    age: f32,
    ttl: f32,
}

#[derive(Default)]
struct ToastQueue {
    items: Vec<Toast>,
}

fn queue_id() -> Id {
    Id::ROOT.with("crusty_toasts")
}

impl Ui<'_> {
    /// Queue an informational toast shown for ~4s.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_kind(text, ToastKind::Info);
    }

    /// Queue a success toast.
    pub fn toast_success(&mut self, text: impl Into<String>) {
        self.toast_kind(text, ToastKind::Success);
    }

    /// Queue an error toast (shown a little longer).
    pub fn toast_error(&mut self, text: impl Into<String>) {
        let ttl = 6.0;
        self.push_toast(text.into(), ToastKind::Error, ttl);
    }

    fn toast_kind(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.push_toast(text.into(), kind, 4.0);
    }

    fn push_toast(&mut self, text: String, kind: ToastKind, ttl: f32) {
        let q = self
            .ctx_mut()
            .memory
            .data_get_or_insert_with::<ToastQueue>(queue_id(), ToastQueue::default);
        q.items.push(Toast {
            text,
            kind,
            age: 0.0,
            ttl,
        });
    }

    /// Advance, render, and reap the toast stack. Call once per frame after
    /// the main UI so toasts float above it.
    pub fn show_toasts(&mut self) {
        let dt = self.ctx().dt();
        // Pull the queue out so we can render without holding the borrow.
        let mut items = match self
            .ctx_mut()
            .memory
            .data_get_mut::<ToastQueue>(queue_id())
        {
            Some(q) => std::mem::take(&mut q.items),
            None => return,
        };
        for t in &mut items {
            t.age += dt;
        }
        items.retain(|t| t.age < t.ttl);

        let style = self.style();
        let screen = self.ctx().screen_rect();
        let margin = 12.0;
        let width = 260.0;
        let pad = Vec2::new(12.0, 8.0);
        let mut y = screen.min.y + margin;

        for t in &items {
            let text_size = self.text_mut().measure(&t.text, style.fonts.body, Some(width - pad.x * 2.0));
            let height = text_size.y + pad.y * 2.0;
            let x = screen.max.x - margin - width;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height));

            // Fade in over the first 0.2s, out over the last 0.5s.
            let fade_in = (t.age / 0.2).clamp(0.0, 1.0);
            let remaining = t.ttl - t.age;
            let fade_out = (remaining / 0.5).clamp(0.0, 1.0);
            let alpha = fade_in.min(fade_out);

            let mut fill = style.palette.surface_hover;
            fill.a *= alpha;
            let mut stroke = style.palette.stroke;
            stroke.a *= alpha;
            self.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
                RectShape::fill(rect, style.rounding.widget, fill)
                    .with_stroke(1.0, stroke)
                    .with_shadow(18.0, Color::rgba(0.0, 0.0, 0.0, 0.4 * alpha)),
            ));

            // Accent stripe down the left edge.
            let mut accent = match t.kind {
                ToastKind::Info => style.palette.accent,
                ToastKind::Success => Color::rgba(0.30, 0.78, 0.45, 1.0),
                ToastKind::Error => Color::rgba(0.90, 0.35, 0.35, 1.0),
            };
            accent.a *= alpha;
            let stripe = Rect::from_min_size(rect.min, Vec2::new(3.0, height));
            self.ctx_mut()
                .overlay_paint
                .push(PaintCmd::Rrect(RectShape::fill(
                    stripe,
                    style.rounding.small,
                    accent,
                )));

            let text_pos = Pos2::new(rect.min.x + pad.x, rect.min.y + pad.y);
            let mut text_color = style.palette.text;
            text_color.a *= alpha;
            let Ui { ctx, text, .. } = self;
            text.add_text(
                &mut ctx.overlay_paint,
                &t.text,
                style.fonts.body,
                text_pos,
                text_color,
                Some(width - pad.x * 2.0),
            );

            y += height + 8.0;
        }

        // Put the (advanced, reaped) queue back.
        let q = self
            .ctx_mut()
            .memory
            .data_get_or_insert_with::<ToastQueue>(queue_id(), ToastQueue::default);
        q.items = items;
    }
}
