use crate::math::{Color, Pos2, Rect, Rounding};
use crate::paint::command::{PaintCmd, RectShape};
use crate::paint::mesh::{DrawRange, Mesh, Vertex};
use crate::paint::TextureId;

const MODE_SOLID: f32 = 0.0;
const MODE_RRECT: f32 = 1.0;
const MODE_SHADOW: f32 = 2.0;
const MODE_GLOW: f32 = 3.0;
const MODE_GLYPH: f32 = 4.0;
const MODE_IMAGE: f32 = 5.0;
const MODE_COLOR_GLYPH: f32 = 6.0;

/// Stateful CPU tessellator. Reused across frames; call [`reset`](Self::reset)
/// each frame before pushing commands.
pub struct Tessellator {
    clip_stack: Vec<Option<Rect>>,
    current_clip: Option<Rect>,
    current_texture: Option<TextureId>,
    current_draw_start: u32,
}

impl Tessellator {
    pub fn new() -> Self {
        Self {
            clip_stack: Vec::new(),
            current_clip: None,
            current_texture: None,
            current_draw_start: 0,
        }
    }

    pub fn reset(&mut self, mesh: &mut Mesh) {
        mesh.clear();
        self.clip_stack.clear();
        self.current_clip = None;
        self.current_texture = None;
        self.current_draw_start = 0;
    }

    pub fn tessellate(&mut self, commands: &[PaintCmd], mesh: &mut Mesh) {
        for cmd in commands {
            match cmd {
                PaintCmd::Rect { rect, color } => {
                    self.push_solid_rect(mesh, *rect, *color);
                }
                PaintCmd::Rrect(shape) => {
                    self.push_rect_shape(mesh, shape);
                }
                PaintCmd::Line {
                    from,
                    to,
                    width,
                    color,
                } => {
                    self.push_line(mesh, *from, *to, *width, *color);
                }
                PaintCmd::ConvexPolygon { points, color } => {
                    self.push_convex_polygon(mesh, points, *color);
                }
                PaintCmd::Glyph {
                    rect,
                    uv_min,
                    uv_max,
                    color,
                } => {
                    self.push_glyph(mesh, *rect, *uv_min, *uv_max, *color);
                }
                PaintCmd::Image {
                    rect,
                    uv_min,
                    uv_max,
                    tint,
                    texture,
                } => {
                    // A different bound texture needs its own draw range so
                    // the backend can rebind the image descriptor.
                    if self.current_texture != Some(*texture) {
                        self.flush_draw(mesh);
                        self.current_texture = Some(*texture);
                    }
                    self.push_textured(mesh, *rect, *uv_min, *uv_max, *tint, MODE_IMAGE);
                }
                PaintCmd::ColorGlyph {
                    rect,
                    uv_min,
                    uv_max,
                    alpha,
                } => {
                    // Color glyphs sample a dedicated atlas binding, so they
                    // never switch `current_texture` — they batch freely with
                    // solid / mask-glyph draws.
                    let tint = Color::WHITE.with_alpha(*alpha);
                    self.push_textured(mesh, *rect, *uv_min, *uv_max, tint, MODE_COLOR_GLYPH);
                }
                PaintCmd::PushClip(r) => {
                    self.flush_draw(mesh);
                    self.clip_stack.push(self.current_clip);
                    let new_clip = match self.current_clip {
                        Some(prev) => prev.intersect(*r),
                        None => *r,
                    };
                    self.current_clip = Some(new_clip);
                }
                PaintCmd::PopClip => {
                    self.flush_draw(mesh);
                    self.current_clip = self.clip_stack.pop().flatten();
                }
            }
        }
        self.flush_draw(mesh);
    }

    fn flush_draw(&mut self, mesh: &mut Mesh) {
        let end = mesh.indices.len() as u32;
        if end > self.current_draw_start {
            mesh.draws.push(DrawRange {
                index_start: self.current_draw_start,
                index_count: end - self.current_draw_start,
                clip: self.current_clip,
                texture: self.current_texture,
            });
            self.current_draw_start = end;
        }
    }

    /// Push a textured quad (image / color glyph) with explicit UVs.
    fn push_textured(
        &mut self,
        mesh: &mut Mesh,
        rect: Rect,
        uv_min: Pos2,
        uv_max: Pos2,
        color: Color,
        mode: f32,
    ) {
        let color_arr = color.to_array();
        let params = [0.0, 0.0, 0.0, mode];
        let base = mesh.vertices.len() as u32;
        let Pos2 { x: x0, y: y0 } = rect.min;
        let Pos2 { x: x1, y: y1 } = rect.max;
        let Pos2 { x: u0, y: v0 } = uv_min;
        let Pos2 { x: u1, y: v1 } = uv_max;
        let mk = |pos: [f32; 2], uv: [f32; 2]| Vertex {
            pos,
            uv,
            color: color_arr,
            params,
            ..Vertex::ZERO
        };
        mesh.vertices.push(mk([x0, y0], [u0, v0]));
        mesh.vertices.push(mk([x1, y0], [u1, v0]));
        mesh.vertices.push(mk([x1, y1], [u1, v1]));
        mesh.vertices.push(mk([x0, y1], [u0, v1]));
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn push_glyph(
        &mut self,
        mesh: &mut Mesh,
        rect: Rect,
        uv_min: Pos2,
        uv_max: Pos2,
        color: Color,
    ) {
        let color_arr = color.to_array();
        let params = [0.0, 0.0, 0.0, MODE_GLYPH];
        let base = mesh.vertices.len() as u32;
        let Pos2 { x: x0, y: y0 } = rect.min;
        let Pos2 { x: x1, y: y1 } = rect.max;
        let Pos2 { x: u0, y: v0 } = uv_min;
        let Pos2 { x: u1, y: v1 } = uv_max;
        let mk = |pos: [f32; 2], uv: [f32; 2]| Vertex {
            pos,
            uv,
            color: color_arr,
            params,
            ..Vertex::ZERO
        };
        mesh.vertices.push(mk([x0, y0], [u0, v0]));
        mesh.vertices.push(mk([x1, y0], [u1, v0]));
        mesh.vertices.push(mk([x1, y1], [u1, v1]));
        mesh.vertices.push(mk([x0, y1], [u0, v1]));
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Emit a rotated quad spanning `from`→`to` with the given stroke
    /// `width`. Drawn in `MODE_SOLID`, which ignores position/uv and just
    /// outputs the vertex color, so an off-axis quad is fine.
    fn push_line(&mut self, mesh: &mut Mesh, from: Pos2, to: Pos2, width: f32, color: Color) {
        let dir = to - from;
        let len = dir.length();
        if len <= 1e-4 || width <= 0.0 {
            return;
        }
        let half = width * 0.5;
        // Square caps: extend each end by half the width along the line so
        // strokes meeting at a corner (e.g. a chevron tip) overlap into a
        // solid point instead of leaving a notch.
        let along = dir / len * half;
        let from = from - along;
        let to = to + along;
        // Perpendicular, scaled to half the stroke width.
        let n = crate::math::Vec2::new(-dir.y, dir.x) / len * half;
        let template = Vertex {
            color: color.to_array(),
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };
        let mk = |p: Pos2| Vertex {
            pos: [p.x, p.y],
            uv: [0.0, 0.0],
            ..template
        };
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(mk(from + n));
        mesh.vertices.push(mk(to + n));
        mesh.vertices.push(mk(to - n));
        mesh.vertices.push(mk(from - n));
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Triangulate a convex polygon as a fan from `points[0]`. Drawn in
    /// `MODE_SOLID` so position/uv are ignored and only the vertex color is
    /// emitted.
    fn push_convex_polygon(&mut self, mesh: &mut Mesh, points: &[Pos2], color: Color) {
        if points.len() < 3 {
            return;
        }
        let template = Vertex {
            color: color.to_array(),
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };
        let base = mesh.vertices.len() as u32;
        for p in points {
            mesh.vertices.push(Vertex {
                pos: [p.x, p.y],
                uv: [0.0, 0.0],
                ..template
            });
        }
        for i in 1..(points.len() as u32 - 1) {
            mesh.indices
                .extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }

    fn push_solid_rect(&mut self, mesh: &mut Mesh, rect: Rect, color: Color) {
        let template = Vertex {
            color: color.to_array(),
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };
        push_quad(mesh, rect, |pos, uv| Vertex {
            pos,
            uv,
            ..template
        });
    }

    fn push_rect_shape(&mut self, mesh: &mut Mesh, shape: &RectShape) {
        let center = shape.rect.center();
        let half = shape.rect.size() * 0.5;
        let rect_center = [center.x, center.y];
        let rect_half = [half.x, half.y];
        let rounding = clamp_rounding(shape.rounding, half.x.min(half.y));
        let rounding_arr = [rounding.nw, rounding.ne, rounding.sw, rounding.se];

        // Order matters for correct alpha blending: back-to-front.
        if let Some(shadow) = shape.shadow {
            // The shadow quad must extend beyond the rect to give the SDF
            // falloff room to fade.
            let pad = shadow.spread * 2.0;
            let expanded = shape.rect.expand(pad);
            let template = Vertex {
                color: shadow.color.to_array(),
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [0.0, shadow.spread, 0.0, MODE_SHADOW],
                ..Vertex::ZERO
            };
            push_quad(mesh, expanded, |pos, uv| Vertex {
                pos,
                uv,
                ..template
            });
        }

        if let Some(glow) = shape.glow {
            let pad = glow.size;
            let expanded = shape.rect.expand(pad);
            let template = Vertex {
                color: glow.color.to_array(),
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [0.0, 0.0, glow.size, MODE_GLOW],
                ..Vertex::ZERO
            };
            push_quad(mesh, expanded, |pos, uv| Vertex {
                pos,
                uv,
                ..template
            });
        }

        if let Some(fill) = shape.fill {
            let stroke_w = shape.stroke.map(|s| s.width).unwrap_or(0.0);
            let stroke_color = shape.stroke.map(|s| s.color).unwrap_or(fill);
            // A combined fill+stroke uses one quad in MODE_RRECT; if no
            // stroke, stroke_w stays 0 and the shader skips that branch.
            let _ = stroke_color; // stroke colour blending lives in Phase 2 (need 2 draws or per-vertex blend)
            let template = Vertex {
                color: fill.to_array(),
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [stroke_w, 0.0, 0.0, MODE_RRECT],
                ..Vertex::ZERO
            };
            push_quad(mesh, shape.rect, |pos, uv| Vertex {
                pos,
                uv,
                ..template
            });
        } else if let Some(stroke) = shape.stroke {
            // Stroke-only: the shader treats `fill` color with full alpha
            // outside the stroke band as transparent.
            let template = Vertex {
                color: stroke.color.to_array(),
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [stroke.width, 0.0, 0.0, MODE_RRECT],
                // Setting color alpha to 0 makes the fill branch contribute
                // nothing; the stroke branch overwrites with full alpha.
                ..Vertex::ZERO
            };
            let mut template = template;
            let mut c = stroke.color.to_array();
            c[3] = 0.0;
            template.color = c;
            push_quad(mesh, shape.rect, |pos, uv| Vertex {
                pos,
                uv,
                ..template
            });
        }
    }
}

impl Default for Tessellator {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_rounding(r: Rounding, max: f32) -> Rounding {
    Rounding {
        nw: r.nw.min(max).max(0.0),
        ne: r.ne.min(max).max(0.0),
        sw: r.sw.min(max).max(0.0),
        se: r.se.min(max).max(0.0),
    }
}

fn push_quad(
    mesh: &mut Mesh,
    rect: Rect,
    make_vertex: impl Fn(/*pos*/ [f32; 2], /*uv*/ [f32; 2]) -> Vertex,
) {
    let base = mesh.vertices.len() as u32;
    let Pos2 { x: x0, y: y0 } = rect.min;
    let Pos2 { x: x1, y: y1 } = rect.max;
    mesh.vertices.push(make_vertex([x0, y0], [0.0, 0.0])); // nw
    mesh.vertices.push(make_vertex([x1, y0], [1.0, 0.0])); // ne
    mesh.vertices.push(make_vertex([x1, y1], [1.0, 1.0])); // se
    mesh.vertices.push(make_vertex([x0, y1], [0.0, 1.0])); // sw
    mesh.indices.extend_from_slice(&[
        base, base + 1, base + 2, base, base + 2, base + 3,
    ]);
}
