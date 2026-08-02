use std::hash::Hasher;

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
    /// A connected stroked path. Kept as a *path* rather than a run of
    /// [`Line`]s so the tessellator can miter the joints and feather the
    /// whole outline as one strip — per-segment quads double-blend at every
    /// joint and cannot be antialiased consistently.
    Path {
        points: Vec<Pos2>,
        /// The last point joins back to the first.
        closed: bool,
        width: f32,
        color: Color,
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
            PaintCmd::Path { color, .. } => color.a *= k,
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

/// Order-sensitive content hash of a paint list.
///
/// Renderers compare it against the previous frame's value to skip
/// re-tessellation and vertex re-upload when nothing changed;
/// [`Context::end_frame`](crate::context::Context::end_frame) uses it to
/// report [`FrameOutput::paint_changed`](crate::context::FrameOutput).
/// Every field of every variant is destructured on purpose so that adding a
/// variant or field breaks this function instead of silently going unhashed.
pub fn paint_hash(cmds: &[PaintCmd]) -> u64 {
    let mut h = PaintHasher(ahash::AHasher::default());
    h.0.write_usize(cmds.len());
    for cmd in cmds {
        match cmd {
            PaintCmd::Rect { rect, color } => {
                h.tag(0);
                h.rect(*rect);
                h.color(*color);
            }
            PaintCmd::Rrect(shape) => {
                let RectShape {
                    rect,
                    rounding: Rounding { nw, ne, sw, se },
                    fill,
                    stroke,
                    shadow,
                    glow,
                } = shape;
                h.tag(1);
                h.rect(*rect);
                for r in [nw, ne, sw, se] {
                    h.f32(*r);
                }
                match fill {
                    Some(c) => {
                        h.tag(1);
                        h.color(*c);
                    }
                    None => h.tag(0),
                }
                match stroke {
                    Some(Stroke { width, color }) => {
                        h.tag(1);
                        h.f32(*width);
                        h.color(*color);
                    }
                    None => h.tag(0),
                }
                match shadow {
                    Some(Shadow { spread, color }) => {
                        h.tag(1);
                        h.f32(*spread);
                        h.color(*color);
                    }
                    None => h.tag(0),
                }
                match glow {
                    Some(Glow { size, color }) => {
                        h.tag(1);
                        h.f32(*size);
                        h.color(*color);
                    }
                    None => h.tag(0),
                }
            }
            PaintCmd::Line {
                from,
                to,
                width,
                color,
            } => {
                h.tag(2);
                h.pos(*from);
                h.pos(*to);
                h.f32(*width);
                h.color(*color);
            }
            PaintCmd::GradientQuad { rect, colors } => {
                h.tag(3);
                h.rect(*rect);
                for c in colors {
                    h.color(*c);
                }
            }
            PaintCmd::Path {
                points,
                closed,
                width,
                color,
            } => {
                h.tag(12);
                h.0.write_usize(points.len());
                for p in points {
                    h.pos(*p);
                }
                h.tag(if *closed { 1 } else { 0 });
                h.f32(*width);
                h.color(*color);
            }
            PaintCmd::ConvexPolygon { points, color } => {
                h.tag(4);
                h.0.write_usize(points.len());
                for p in points {
                    h.pos(*p);
                }
                h.color(*color);
            }
            PaintCmd::Glyph {
                rect,
                uv_min,
                uv_max,
                color,
            } => {
                h.tag(5);
                h.rect(*rect);
                h.pos(*uv_min);
                h.pos(*uv_max);
                h.color(*color);
            }
            PaintCmd::Image {
                rect,
                uv_min,
                uv_max,
                tint,
                texture,
            } => {
                h.tag(6);
                h.rect(*rect);
                h.pos(*uv_min);
                h.pos(*uv_max);
                h.color(*tint);
                h.0.write_u64(texture.0);
            }
            PaintCmd::ColorGlyph {
                rect,
                uv_min,
                uv_max,
                alpha,
            } => {
                h.tag(7);
                h.rect(*rect);
                h.pos(*uv_min);
                h.pos(*uv_max);
                h.f32(*alpha);
            }
            PaintCmd::Mesh { vertices, indices } => {
                h.tag(8);
                h.0.write_usize(vertices.len());
                for (p, c) in vertices {
                    h.pos(*p);
                    h.color(*c);
                }
                h.0.write_usize(indices.len());
                for i in indices {
                    h.0.write_u32(*i);
                }
            }
            PaintCmd::PushClip(rect) => {
                h.tag(9);
                h.rect(*rect);
            }
            PaintCmd::PopClip => h.tag(10),
        }
    }
    h.0.finish()
}

struct PaintHasher(ahash::AHasher);

impl PaintHasher {
    fn tag(&mut self, t: u8) {
        self.0.write_u8(t);
    }
    fn f32(&mut self, v: f32) {
        self.0.write_u32(v.to_bits());
    }
    fn pos(&mut self, p: Pos2) {
        self.f32(p.x);
        self.f32(p.y);
    }
    fn rect(&mut self, r: Rect) {
        self.pos(r.min);
        self.pos(r.max);
    }
    fn color(&mut self, c: Color) {
        self.f32(c.r);
        self.f32(c.g);
        self.f32(c.b);
        self.f32(c.a);
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
