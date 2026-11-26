use crate::animation::approach_half_life;
use crate::context::Ui;
use crate::math::{Color, Pos2, Vec2};
use crate::paint::{PaintCmd, TextureId};
use crate::response::Response;

/// Displays a registered [`TextureId`] at a fixed size, with optional UV
/// sub-rect (for atlases / sprite sheets) and a multiplicative tint.
pub struct Image {
    texture: TextureId,
    size: Vec2,
    uv_min: Pos2,
    uv_max: Pos2,
    tint: Color,
}

impl Image {
    pub fn new(texture: TextureId, size: Vec2) -> Self {
        Self {
            texture,
            size,
            uv_min: Pos2::new(0.0, 0.0),
            uv_max: Pos2::new(1.0, 1.0),
            tint: Color::WHITE,
        }
    }

    /// Sample a sub-rectangle of the texture (normalised 0..1 UVs).
    pub fn uv(mut self, min: Pos2, max: Pos2) -> Self {
        self.uv_min = min;
        self.uv_max = max;
        self
    }

    /// Multiply the sampled color (white = unchanged).
    pub fn tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(self.texture.0);
        let rect = ui.allocate(self.size);

        let hovered = ui.contains_pointer(rect);
        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim = approach_half_life(
            mem.hover_anim,
            if hovered { 1.0 } else { 0.0 },
            dt,
            style.motion.hover_half_life,
        );
        let hover_anim = mem.hover_anim;

        ui.ctx_mut().paint.push(PaintCmd::Image {
            rect,
            uv_min: self.uv_min,
            uv_max: self.uv_max,
            tint: self.tint,
            texture: self.texture,
        });

        Response {
            id,
            rect,
            hovered,
            pressed: false,
            clicked: false,
            hover_anim,
            press_anim: 0.0,
        }
    }
}

/// A clickable [`Image`]: dims when idle, brightens on hover, sinks on
/// press. Returns a [`Response`] whose `clicked` reports activation.
pub struct ImageButton {
    image: Image,
}

impl ImageButton {
    pub fn new(texture: TextureId, size: Vec2) -> Self {
        Self {
            image: Image::new(texture, size),
        }
    }

    pub fn uv(mut self, min: Pos2, max: Pos2) -> Self {
        self.image = self.image.uv(min, max);
        self
    }

    pub fn tint(mut self, tint: Color) -> Self {
        self.image = self.image.tint(tint);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(self.image.texture.0);
        let rect = ui.allocate(self.image.size);

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
        mem.hover_anim = approach_half_life(
            mem.hover_anim,
            if hovered { 1.0 } else { 0.0 },
            dt,
            style.motion.hover_half_life,
        );
        mem.press_anim = approach_half_life(
            mem.press_anim,
            if pressed { 1.0 } else { 0.0 },
            dt,
            style.motion.press_half_life,
        );
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        // Idle is slightly dimmed; hover lifts to full, press dims again.
        let brightness = 0.82 + 0.18 * hover_anim - 0.15 * press_anim;
        let base = self.image.tint;
        let tint = Color::rgba(
            base.r * brightness,
            base.g * brightness,
            base.b * brightness,
            base.a,
        );

        ui.ctx_mut().paint.push(PaintCmd::Image {
            rect,
            uv_min: self.image.uv_min,
            uv_max: self.image.uv_max,
            tint,
            texture: self.image.texture,
        });

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
