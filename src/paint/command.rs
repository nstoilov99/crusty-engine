use crate::math::{Color, Pos2, Rect, Rounding};
use crate::paint::TextureId;

/// A single drawing command emitted by widgets and consumed by the
/// tessellator. Phase 1 modelled SDF primitives; Phase 2 added [`Glyph`]
/// for sampling rasterised text out of the GPU glyph atlas; Phase 10 adds
/// [`Image`] (user textures) and [`ColorGlyph`] (the RGBA glyph atlas).
#[derive(Clone, Debug)]
pub enum PaintCmd {
    /// Filled axis-aligned rect with no rounding. Useful for backgrounds.
    Rect {
        rect: Rect,
        color: Color,
    },
    /// Generic rounded-rect with fill / stroke / shadow / glow.
    Rrect(RectShape),
    /// A straight stroke of the given `width` between two points, square
    /// caps. Used for thin UI chrome (chevrons, separators) that shouldn't
    /// depend on font glyphs.
    Line {
        from: Pos2,
        to: Pos2,
        width: f32,
        color: Color,
    },
    /// Textured quad sampling the glyph atlas as an alpha mask. The
    /// `TextRenderer` emits these for every shaped glyph.
    Glyph {
        rect: Rect,
        uv_min: Pos2,
        uv_max: Pos2,
        color: Color,
    },
    /// Textured quad sampling a user-registered [`TextureId`]. `tint`
    /// multiplies the sampled RGBA (use [`Color::WHITE`] for no tint).
    Image {
        rect: Rect,
        uv_min: Pos2,
        uv_max: Pos2,
        tint: Color,
        texture: TextureId,
    },
    /// Textured quad sampling the RGBA color-glyph atlas (color emoji).
    /// `alpha` scales the sampled alpha for fades; RGB is left untinted.
    ColorGlyph {
        rect: Rect,
        uv_min: Pos2,
        uv_max: Pos2,
        alpha: f32,
    },
    /// Push a clip rect; subsequent commands are scissored to its intersection
    /// with the current clip until [`PaintCmd::PopClip`].
    PushClip(Rect),
    PopClip,
}

/// Fully-parameterised rounded rectangle. Fields are optional and combined
/// in a fixed order during tessellation: shadow, glow, fill, stroke.
#[derive(Clone, Debug)]
pub struct RectShape {
    pub rect: Rect,
    pub rounding: Rounding,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub shadow: Option<Shadow>,
    pub glow: Option<Glow>,
}

#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    /// Soft falloff radius in pixels.
    pub spread: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct Glow {
    /// Falloff radius in pixels.
    pub size: f32,
    pub color: Color,
}

impl RectShape {
    pub fn fill(rect: Rect, rounding: impl Into<Rounding>, color: Color) -> Self {
        Self {
            rect,
            rounding: rounding.into(),
            fill: Some(color),
            stroke: None,
            shadow: None,
            glow: None,
        }
    }

    pub fn with_stroke(mut self, width: f32, color: Color) -> Self {
        self.stroke = Some(Stroke { width, color });
        self
    }

    pub fn with_shadow(mut self, spread: f32, color: Color) -> Self {
        self.shadow = Some(Shadow { spread, color });
        self
    }

    pub fn with_glow(mut self, size: f32, color: Color) -> Self {
        self.glow = Some(Glow { size, color });
        self
    }
}
