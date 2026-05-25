use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

pub struct ProgressBar {
    progress: f32,
    width: Option<f32>,
    smooth: bool,
}

impl ProgressBar {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            width: None,
            smooth: true,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id("progress_bar");
        let width = self.width.unwrap_or(style.sizes.progress_default_width);
        let height = style.sizes.progress_height;
        let rect = ui.allocate(Vec2::new(width, height));

        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        let displayed = if self.smooth {
            mem.aux_anim = approach_half_life(mem.aux_anim, self.progress, dt, 0.18);
            mem.aux_anim
        } else {
            mem.aux_anim = self.progress;
            self.progress
        };

        let track_shape = RectShape::fill(
            rect,
            Rounding::same(height * 0.5),
            style.palette.surface_active,
        );
        ui.ctx_mut().paint.push(PaintCmd::Rrect(track_shape));

        let filled_w = (rect.width() * displayed.clamp(0.0, 1.0)).max(0.0);
        if filled_w > 0.5 {
            let filled = Rect::from_min_size(rect.min, Vec2::new(filled_w, rect.height()));
            let fill_shape =
                RectShape::fill(filled, Rounding::same(height * 0.5), style.palette.success);
            ui.ctx_mut().paint.push(PaintCmd::Rrect(fill_shape));
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
