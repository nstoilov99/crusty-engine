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
    /// Axis-aligned quad with one color per corner, interpolated across
    /// the surface (color pickers, gradient bars). Corner order:
    /// top-left, top-right, bottom-right, bottom-left. Interpolation is
    /// barycentric per-triangle, not bilinear — subdivide caller-side when
    /// both axes vary (e.g. an SV square) to hide the diagonal seam.
    GradientQuad {
        rect: Rect,
        colors: [Color; 4],
    },
    /// A solid-filled **convex** polygon, triangulated as a fan from the
    /// first vertex. Points must wind consistently and describe a convex
    /// outline (circles, triangles, sampled bezier fills, gizmo handles).
    /// Concave shapes will render incorrectly — split them caller-side.
    ConvexPolygon {
        points: Vec<Pos2>,
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
    /// Pre-triangulated indexed mesh with per-vertex colors, drawn flat
    /// (no texture). Escape hatch for externally tessellated geometry —
    /// e.g. 3D transform-gizmo draw data — that is neither convex nor
    /// single-colored. Indices reference `vertices`; triangle winding is
    /// not checked.
    Mesh {
        vertices: Vec<(Pos2, Color)>,
        indices: Vec<u32>,
    },
    /// Push a clip rect; subsequent commands are scissored to its intersection
    /// with the current clip until [`PaintCmd::PopClip`].
    PushClip(Rect),
    PopClip,
}

/// Multiply the alpha of every colour in a slice of paint commands by `k`.
/// Fades a self-contained block (e.g. a drag decorator) as one unit without
/// per-shape edits or a tessellator-level layer-alpha pass.
pub fn scale_alpha(cmds: &mut [PaintCmd], k: f32) {
    for cmd in cmds {
        match cmd {
            PaintCmd::Rect { color, .. } => color.a *= k,
            PaintCmd::GradientQuad { colors, .. } => {
                for c in colors {
                    c.a *= k;
                }
            }
            PaintCmd::Line { color, .. } => color.a *= k,
            PaintCmd::ConvexPolygon { color, .. } => color.a *= k,
            PaintCmd::Mesh { vertices, .. } => {
                for (_, c) in vertices {
                    c.a *= k;
                }
            }
            PaintCmd::Glyph { color, .. } => color.a *= k,
            PaintCmd::Image { tint, .. } => tint.a *= k,
            PaintCmd::ColorGlyph { alpha, .. } => *alpha *= k,
            PaintCmd::Rrect(shape) => {
                if let Some(c) = shape.fill.as_mut() {
                    c.a *= k;
                }
                if let Some(s) = shape.stroke.as_mut() {
                    s.color.a *= k;
                }
                if let Some(s) = shape.shadow.as_mut() {
                    s.color.a *= k;
                }
                if let Some(g) = shape.glow.as_mut() {
                    g.color.a *= k;
                }
            }
            PaintCmd::PushClip(_) | PaintCmd::PopClip => {}
        }
    }
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
