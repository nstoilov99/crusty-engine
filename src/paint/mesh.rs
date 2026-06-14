use bytemuck::{Pod, Zeroable};

use crate::math::Rect;
use crate::paint::TextureId;

/// One vertex of a tessellated UI mesh. Layout must match `ui.wgsl`.
///
/// The fragment shader's interpretation of `params.w` (mode):
/// `0 = solid`, `1 = rrect fill/stroke`, `2 = shadow`, `3 = glow`,
/// `4 = glyph (R8 atlas)`, `5 = image (user texture)`,
/// `6 = color glyph (RGBA atlas)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub rect_center: [f32; 2],
    pub rect_half: [f32; 2],
    pub rounding: [f32; 4],
    /// `[stroke_w, shadow_radius, glow_radius, mode]`.
    pub params: [f32; 4],
}

impl Vertex {
    pub const ZERO: Self = Self {
        pos: [0.0; 2],
        uv: [0.0; 2],
        color: [0.0; 4],
        rect_center: [0.0; 2],
        rect_half: [0.0; 2],
        rounding: [0.0; 4],
        params: [0.0; 4],
    };
}

/// A CPU-side mesh produced by the [`Tessellator`](super::Tessellator).
/// Backends upload these to GPU buffers and issue per-`DrawRange` draw
/// calls so clip changes can become scissor rectangles.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Sub-ranges into `indices`. Each range has its own scissor; backends
    /// emit one `cmd_draw_indexed` per range.
    pub draws: Vec<DrawRange>,
}

#[derive(Clone, Copy, Debug)]
pub struct DrawRange {
    pub index_start: u32,
    pub index_count: u32,
    /// `None` means "no scissor — use the full framebuffer".
    pub clip: Option<Rect>,
    /// User texture bound for `MODE_IMAGE` draws in this range. `None`
    /// binds a dummy image; non-image modes ignore it.
    pub texture: Option<TextureId>,
}

impl Mesh {
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.draws.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}
