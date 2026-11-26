use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

pub struct ProgressBar {
    progress: f32,
    width: Option<f32>,
    height: Option<f32>,
    smooth: bool,
    fill: Option<Color>,
    text: Option<String>,
    indeterminate: bool,
}

impl ProgressBar {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            width: None,
            height: None,
            smooth: true,
            fill: None,
            text: None,
            indeterminate: false,
        }
    }

    /// Unknown duration: a 25%-width accent segment sweeps the track.
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Override the fill color (defaults to the palette's success color).
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Text centered on the bar (egui `ProgressBar::text` parity).
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id("progress_bar");
        let width = self.width.unwrap_or(style.sizes.progress_default_width);
        let height = self.height.unwrap_or(style.sizes.progress_height);
        let rect = ui.allocate(Vec2::new(width, height));

        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        let displayed = if self.indeterminate {
            // aux_anim doubles as the sweep phase (wraps at 1).
            mem.aux_anim = (mem.aux_anim + dt * 0.8).fract();
            mem.aux_anim
        } else if self.smooth {
            mem.aux_anim = approach_half_life(mem.aux_anim, self.progress, dt, 0.18);
            mem.aux_anim
        } else {
            mem.aux_anim = self.progress;
            self.progress
        };

        // Track reads inset: darkest surface, no border.
        let track_shape = RectShape::fill(rect, Rounding::same(height * 0.5), style.palette.input);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(track_shape));

        let fill_color = self.fill.unwrap_or(style.palette.accent_active);
        if self.indeterminate {
            let seg_w = rect.width() * 0.25;
            // Sweep the segment fully across: enters from the left edge,
            // exits at the right, clipped to the track.
            let x = rect.min.x - seg_w + (rect.width() + seg_w) * displayed;
            let seg = Rect::from_min_size(
                Pos2::new(x.max(rect.min.x), rect.min.y),
                Vec2::new(
                    (x + seg_w).min(rect.max.x) - x.max(rect.min.x),
                    rect.height(),
                ),
            );
            if seg.width() > 0.5 {
                ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                    seg,
                    Rounding::same(height * 0.5),
                    fill_color,
                )));
            }
        } else {
            let filled_w = (rect.width() * displayed.clamp(0.0, 1.0)).max(0.0);
            if filled_w > 0.5 {
                let filled = Rect::from_min_size(rect.min, Vec2::new(filled_w, rect.height()));
                let fill_shape = RectShape::fill(filled, Rounding::same(height * 0.5), fill_color);
                ui.ctx_mut().paint.push(PaintCmd::Rrect(fill_shape));
            }
        }

        if let Some(text) = &self.text {
            let font = style.fonts.small.min(height - 2.0);
            let size = ui.text_mut().measure(text, font, None);
            let pos = Pos2::new(
                rect.center().x - size.x * 0.5,
                rect.center().y - size.y * 0.5,
            );
            let Ui { ctx, text: tr, .. } = ui;
            tr.add_text(&mut ctx.paint, text, font, pos, style.palette.text, None);
        }

        Response {
            id,
            rect,
            hovered: false,
            pressed: false,
            clicked: false,
            hover_anim: 0.0,
            press_anim: 0.0,
        }
    }
}
