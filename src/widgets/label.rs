use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::Color;
use crate::response::Response;

pub struct Label {
    text: String,
    size: Option<f32>,
    color: Option<Color>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: None,
            color: None,
        }
    }

    pub fn size(mut self, size_px: f32) -> Self {
        self.size = Some(size_px);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let size = self.size.unwrap_or(style.fonts.body);
        let color = self.color.unwrap_or(style.palette.text);

        let id = ui.alloc_id(&self.text);
        let max_width = Some(ui.available().width());
        let measured = ui.text_mut().measure(&self.text, size, max_width);
        let rect = ui.allocate(measured);

        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim = approach_half_life(mem.hover_anim, 0.0, dt, style.motion.hover_half_life);
        mem.press_anim = approach_half_life(mem.press_anim, 0.0, dt, style.motion.press_half_life);
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        let Ui { ctx, text, .. } = ui;
        text.add_text(&mut ctx.paint, &self.text, size, rect.min, color, max_width);

        Response {
            id,
            rect,
            hovered: false,
            pressed: false,
            clicked: false,
            hover_anim,
            press_anim,
        }
    }
}
