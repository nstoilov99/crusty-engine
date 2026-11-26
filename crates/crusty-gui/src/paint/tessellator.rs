use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::TextureId;
use crate::paint::command::{PaintCmd, RectShape};
use crate::paint::mesh::{DrawRange, Mesh, Vertex};

const MODE_SOLID: f32 = 0.0;
const MODE_RRECT: f32 = 1.0;
const MODE_SHADOW: f32 = 2.0;
const MODE_GLOW: f32 = 3.0;
const MODE_GLYPH: f32 = 4.0;
const MODE_IMAGE: f32 = 5.0;
const MODE_COLOR_GLYPH: f32 = 6.0;
/// Rrect whose fill alpha is real opacity, not glass tint strength.
const MODE_RRECT_ALPHA: f32 = 7.0;

/// Width of the alpha ramp added around solid geometry, in physical pixels.
///
/// The UI pass has no MSAA and `MODE_SOLID` returns its vertex color flat, so
/// a diagonal edge is a hard staircase. Rounded rects escape this because the
/// shader antialiases them analytically from an SDF — everything else needs
/// coverage baked into the mesh, which is what feathering is.
///
/// 1px is the egui value and the right one at `pixels_per_point = 1.0`: the
/// ramp spans exactly the pixel the edge passes through.
pub const DEFAULT_FEATHERING: f32 = 1.0;

/// Stateful CPU tessellator. Reused across frames; call [`reset`](Self::reset)
/// each frame before pushing commands.
pub struct Tessellator {
    clip_stack: Vec<Option<Rect>>,
    current_clip: Option<Rect>,
    current_texture: Option<TextureId>,
    current_draw_start: u32,
    /// Alpha-ramp width in physical pixels. `0.0` disables feathering — the
    /// escape hatch for tests that want exact pixel comparisons and for
    /// callers doing their own AA.
    feathering: f32,
}

impl Tessellator {
    pub fn new() -> Self {
        Self {
            clip_stack: Vec::new(),
            current_clip: None,
            current_texture: None,
            current_draw_start: 0,
            feathering: DEFAULT_FEATHERING,
        }
    }

    /// Override the feather width. `0.0` produces the old hard-edged output.
    pub fn with_feathering(mut self, feathering: f32) -> Self {
        self.feathering = feathering.max(0.0);
        self
    }

    pub fn set_feathering(&mut self, feathering: f32) {
        self.feathering = feathering.max(0.0);
    }

    pub fn feathering(&self) -> f32 {
        self.feathering
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
                PaintCmd::GradientQuad { rect, colors } => {
                    self.push_gradient_quad(mesh, *rect, *colors);
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
                PaintCmd::Path {
                    points,
                    closed,
                    width,
                    color,
                } => {
                    self.push_path_stroke(mesh, points, *closed, *width, *color);
                }
                PaintCmd::ConvexPolygon { points, color } => {
                    self.push_convex_polygon(mesh, points, *color);
                }
                PaintCmd::Mesh { vertices, indices } => {
                    self.push_mesh(mesh, vertices, indices);
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

    /// A single segment: the two-point case of [`push_path_stroke`].
    fn push_line(&mut self, mesh: &mut Mesh, from: Pos2, to: Pos2, width: f32, color: Color) {
        self.push_path_stroke(mesh, &[from, to], false, width, color);
    }

    /// Stroke a connected path, feathered.
    ///
    /// One strip for the whole path rather than a quad per segment, which
    /// gets three things at once: miter joins (no notch, no double-blend at
    /// interior points), a single alpha ramp along both flanks, and fewer
    /// vertices than the overlapping-quad approach it replaces.
    ///
    /// Layout across the stroke, from one flank to the other:
    /// `α0 … α1 (core) … α0`, with each ramp `feathering` wide. The core
    /// therefore spans `width - feathering`, so the *visual* edge — where
    /// coverage crosses 50% — still lands at `±width/2`.
    ///
    /// **Thin strokes.** Below one feather width there is no core left. Rather
    /// than let the geometry invert, the stroke collapses to a single ramp of
    /// exactly `feathering` px whose peak alpha is `width / feathering`
    /// (egui's trick): a 0.4px line renders as a 1px line at 40% alpha, which
    /// is what it should look like, and it fades smoothly instead of
    /// flickering in and out as it crosses the pixel grid.
    fn push_path_stroke(
        &mut self,
        mesh: &mut Mesh,
        points: &[Pos2],
        closed: bool,
        width: f32,
        color: Color,
    ) {
        if width <= 0.0 {
            return;
        }
        let pts = dedup_path(points, closed);
        if pts.len() < 2 {
            return;
        }
        let feather = self.feathering;
        if feather <= 0.0 {
            self.push_path_stroke_hard(mesh, &pts, closed, width, color);
            return;
        }

        // Thin-stroke collapse: one ramp, alpha scaled by how much of it the
        // stroke actually covers.
        let (core_half, ramp, alpha_scale) = if width < feather {
            (0.0_f32, feather * 0.5, width / feather)
        } else {
            ((width - feather) * 0.5, feather, 1.0)
        };

        let normals = path_normals(&pts, closed);
        let mut c = color.to_array();
        c[3] *= alpha_scale;
        let solid = c;
        let clear = [c[0], c[1], c[2], 0.0];

        let base = mesh.vertices.len() as u32;
        let mk = |p: Pos2, col: [f32; 4]| Vertex {
            pos: [p.x, p.y],
            uv: [0.0, 0.0],
            color: col,
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };

        // Four rings per point: outer-clear, outer-core, inner-core,
        // inner-clear. With a zero-width core the two core rings coincide,
        // which is exactly the thin-line ramp.
        for (p, n) in pts.iter().zip(normals.iter()) {
            let out_e = *p + *n * (core_half + ramp);
            let out_c = *p + *n * core_half;
            let in_c = *p - *n * core_half;
            let in_e = *p - *n * (core_half + ramp);
            mesh.vertices.push(mk(out_e, clear));
            mesh.vertices.push(mk(out_c, solid));
            mesh.vertices.push(mk(in_c, solid));
            mesh.vertices.push(mk(in_e, clear));
        }

        let n_pts = pts.len() as u32;
        let last = if closed { n_pts } else { n_pts - 1 };
        for i in 0..last {
            let a = base + (i % n_pts) * 4;
            let b = base + ((i + 1) % n_pts) * 4;
            // Three quads: outer ramp, core, inner ramp.
            for k in 0..3u32 {
                mesh.indices.extend_from_slice(&[
                    a + k,
                    b + k,
                    b + k + 1,
                    a + k,
                    b + k + 1,
                    a + k + 1,
                ]);
            }
        }
    }

    /// The un-feathered path stroke, used when feathering is disabled.
    fn push_path_stroke_hard(
        &mut self,
        mesh: &mut Mesh,
        pts: &[Pos2],
        closed: bool,
        width: f32,
        color: Color,
    ) {
        let half = width * 0.5;
        let normals = path_normals(pts, closed);
        let base = mesh.vertices.len() as u32;
        let template = Vertex {
            color: color.to_array(),
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };
        for (p, n) in pts.iter().zip(normals.iter()) {
            for side in [1.0_f32, -1.0] {
                let q = *p + *n * (half * side);
                mesh.vertices.push(Vertex {
                    pos: [q.x, q.y],
                    uv: [0.0, 0.0],
                    ..template
                });
            }
        }
        let n_pts = pts.len() as u32;
        let last = if closed { n_pts } else { n_pts - 1 };
        for i in 0..last {
            let a = base + (i % n_pts) * 2;
            let b = base + ((i + 1) % n_pts) * 2;
            mesh.indices
                .extend_from_slice(&[a, b, b + 1, a, b + 1, a + 1]);
        }
    }

    /// Triangulate a convex polygon as a fan, with a feathered rim.
    ///
    /// The fan is inset by half a feather and a ramp ring runs from there out
    /// to half a feather beyond the original outline, so the shape keeps its
    /// size — coverage crosses 50% exactly on the outline the caller asked
    /// for. This is what turns a 20-gon pin dot into something that reads as
    /// a circle rather than a faceted disc.
    fn push_convex_polygon(&mut self, mesh: &mut Mesh, points: &[Pos2], color: Color) {
        if points.len() < 3 {
            return;
        }
        let feather = self.feathering;
        let template = Vertex {
            color: color.to_array(),
            params: [0.0, 0.0, 0.0, MODE_SOLID],
            ..Vertex::ZERO
        };
        let base = mesh.vertices.len() as u32;

        if feather <= 0.0 {
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
            return;
        }

        // Outward normals. Which way `path_normals` points depends on the
        // caller's winding, so rather than reason about it, measure: for a
        // convex shape, the outward side is the one that moves a vertex away
        // from the centroid.
        let normals = path_normals(points, true);
        let outward = outward_sign(points, &normals);
        let half = feather * 0.5;
        let solid = color.to_array();
        let clear = [solid[0], solid[1], solid[2], 0.0];

        for (p, n) in points.iter().zip(normals.iter()) {
            let inner = *p - *n * (half * outward);
            let outer = *p + *n * (half * outward);
            mesh.vertices.push(Vertex {
                pos: [inner.x, inner.y],
                uv: [0.0, 0.0],
                color: solid,
                ..template
            });
            mesh.vertices.push(Vertex {
                pos: [outer.x, outer.y],
                uv: [0.0, 0.0],
                color: clear,
                ..template
            });
        }

        let n_pts = points.len() as u32;
        // Fan over the inset ring…
        for i in 1..(n_pts - 1) {
            mesh.indices
                .extend_from_slice(&[base, base + i * 2, base + (i + 1) * 2]);
        }
        // …then the ramp ring around it.
        for i in 0..n_pts {
            let a = base + i * 2;
            let b = base + ((i + 1) % n_pts) * 2;
            mesh.indices
                .extend_from_slice(&[a, b, b + 1, a, b + 1, a + 1]);
        }
    }

    /// Copy pre-triangulated geometry (per-vertex colors) in `MODE_SOLID`.
    fn push_mesh(&mut self, mesh: &mut Mesh, vertices: &[(Pos2, Color)], indices: &[u32]) {
        let base = mesh.vertices.len() as u32;
        for (p, c) in vertices {
            mesh.vertices.push(Vertex {
                pos: [p.x, p.y],
                uv: [0.0, 0.0],
                color: c.to_array(),
                params: [0.0, 0.0, 0.0, MODE_SOLID],
                ..Vertex::ZERO
            });
        }
        mesh.indices.extend(indices.iter().map(|i| base + i));
    }

    /// Quad with per-corner colors (tl, tr, br, bl) in `MODE_SOLID`.
    fn push_gradient_quad(&mut self, mesh: &mut Mesh, rect: Rect, colors: [Color; 4]) {
        let base = mesh.vertices.len() as u32;
        let corners = [
            ([rect.min.x, rect.min.y], colors[0]),
            ([rect.max.x, rect.min.y], colors[1]),
            ([rect.max.x, rect.max.y], colors[2]),
            ([rect.min.x, rect.max.y], colors[3]),
        ];
        for (pos, color) in corners {
            mesh.vertices.push(Vertex {
                pos,
                uv: [0.0, 0.0],
                color: color.to_array(),
                params: [0.0, 0.0, 0.0, MODE_SOLID],
                ..Vertex::ZERO
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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
            let mode = if shape.translucent {
                MODE_RRECT_ALPHA
            } else {
                MODE_RRECT
            };
            let template = Vertex {
                color: fill.to_array(),
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [stroke_w, 0.0, 0.0, mode],
                stroke_color: stroke_color.to_array(),
                ..Vertex::ZERO
            };
            push_quad(mesh, shape.rect, |pos, uv| Vertex {
                pos,
                uv,
                ..template
            });
        } else if let Some(stroke) = shape.stroke {
            // Stroke-only: params.y = 1 tells the shader to keep the
            // interior fully transparent (no glass/backdrop compositing).
            let mut c = stroke.color.to_array();
            c[3] = 0.0;
            let template = Vertex {
                color: c,
                rect_center,
                rect_half,
                rounding: rounding_arr,
                params: [stroke.width, 1.0, 0.0, MODE_RRECT],
                stroke_color: stroke.color.to_array(),
                ..Vertex::ZERO
            };
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

/// Drop consecutive duplicate points (and, on a closed path, a final point
/// equal to the first). Zero-length segments have no direction, so a normal
/// computed from one is `NaN` — every path helper here assumes this ran.
fn dedup_path(points: &[Pos2], closed: bool) -> Vec<Pos2> {
    const EPS: f32 = 1e-6;
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len());
    for p in points {
        if !p.x.is_finite() || !p.y.is_finite() {
            continue;
        }
        match out.last() {
            Some(q) if (p.x - q.x).abs() <= EPS && (p.y - q.y).abs() <= EPS => {}
            _ => out.push(*p),
        }
    }
    if closed && out.len() > 1 {
        let (first, last) = (out[0], out[out.len() - 1]);
        if (first.x - last.x).abs() <= EPS && (first.y - last.y).abs() <= EPS {
            out.pop();
        }
    }
    out
}

/// Per-point unit normals for a path.
///
/// Interior points get the **miter** normal — the angle bisector, scaled by
/// `1/cos(θ/2)` so both flanks stay parallel to their segments and the joint
/// neither gaps nor overlaps. The scale is clamped: at a hairpin the exact
/// miter runs to infinity, and a bevel-ish stub is the right answer there.
/// Endpoints of an open path just take their one segment's normal.
fn path_normals(points: &[Pos2], closed: bool) -> Vec<Vec2> {
    /// Past this the miter is clipped. 4x the half-width is the usual
    /// choice: it survives a 30-degree turn intact and stops a hairpin from
    /// launching a spike across the screen.
    const MAX_MITER: f32 = 4.0;
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    if n < 2 {
        return vec![Vec2::new(0.0, 0.0); n];
    }
    let seg_normal = |a: Pos2, b: Pos2| -> Vec2 {
        let d = b - a;
        let len = d.length();
        if len <= 1e-6 {
            Vec2::new(0.0, 0.0)
        } else {
            Vec2::new(-d.y, d.x) / len
        }
    };
    for i in 0..n {
        let prev = if i == 0 {
            if closed {
                Some(seg_normal(points[n - 1], points[0]))
            } else {
                None
            }
        } else {
            Some(seg_normal(points[i - 1], points[i]))
        };
        let next = if i + 1 < n {
            Some(seg_normal(points[i], points[i + 1]))
        } else if closed {
            Some(seg_normal(points[n - 1], points[0]))
        } else {
            None
        };
        let v = match (prev, next) {
            (Some(a), Some(b)) => {
                let sum = Vec2::new(a.x + b.x, a.y + b.y);
                let len = sum.length();
                if len <= 1e-6 {
                    // Exact reversal (a hairpin): no bisector exists.
                    a
                } else {
                    let dir = sum / len;
                    // cos(theta/2) = dir . a; the miter scale is its inverse.
                    let cos_half = (dir.x * a.x + dir.y * a.y).abs().max(1e-3);
                    dir * (1.0 / cos_half).min(MAX_MITER)
                }
            }
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => Vec2::new(0.0, 0.0),
        };
        out.push(v);
    }
    out
}

/// `+1` if `normals` point away from the polygon's centroid, `-1` if toward.
///
/// Measured rather than derived from winding: a caller may hand us either
/// orientation, and screen space being y-down makes the usual signed-area
/// argument easy to get backwards. For a convex outline "further from the
/// centroid" is exactly "outward", so this is both simpler and harder to get
/// wrong. Summed over every vertex so one degenerate normal cannot decide it.
fn outward_sign(points: &[Pos2], normals: &[Vec2]) -> f32 {
    let n = points.len().min(normals.len());
    if n == 0 {
        return 1.0;
    }
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    for p in &points[..n] {
        cx += p.x;
        cy += p.y;
    }
    let (cx, cy) = (cx / n as f32, cy / n as f32);
    let mut vote = 0.0f32;
    for i in 0..n {
        let (dx, dy) = (points[i].x - cx, points[i].y - cy);
        vote += dx * normals[i].x + dy * normals[i].y;
    }
    if vote >= 0.0 { 1.0 } else { -1.0 }
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
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample the alpha a feathered mesh produces along a vertical cut at
    /// `x`, by finding which triangle covers each y and interpolating.
    fn alpha_profile(mesh: &Mesh, x: f32, y0: f32, y1: f32, steps: usize) -> Vec<(f32, f32)> {
        (0..=steps)
            .map(|i| {
                let y = y0 + (y1 - y0) * i as f32 / steps as f32;
                let mut a = 0.0f32;
                for tri in mesh.indices.chunks(3) {
                    let v: Vec<&Vertex> = tri.iter().map(|i| &mesh.vertices[*i as usize]).collect();
                    let (p0, p1, p2) = (v[0].pos, v[1].pos, v[2].pos);
                    let d = (p1[1] - p2[1]) * (p0[0] - p2[0]) + (p2[0] - p1[0]) * (p0[1] - p2[1]);
                    if d.abs() < 1e-9 {
                        continue;
                    }
                    let l0 = ((p1[1] - p2[1]) * (x - p2[0]) + (p2[0] - p1[0]) * (y - p2[1])) / d;
                    let l1 = ((p2[1] - p0[1]) * (x - p2[0]) + (p0[0] - p2[0]) * (y - p2[1])) / d;
                    let l2 = 1.0 - l0 - l1;
                    if l0 >= -1e-4 && l1 >= -1e-4 && l2 >= -1e-4 {
                        a = a.max(l0 * v[0].color[3] + l1 * v[1].color[3] + l2 * v[2].color[3]);
                    }
                }
                (y, a)
            })
            .collect()
    }

    fn tess(cmds: &[PaintCmd]) -> Mesh {
        let mut t = Tessellator::new();
        let mut m = Mesh::default();
        t.reset(&mut m);
        t.tessellate(cmds, &mut m);
        m
    }

    /// A feathered stroke ramps 0 -> 1 -> 0 across its width, and the 50%
    /// crossings land on the edges the caller asked for.
    #[test]
    fn feathered_line_has_a_ramped_cross_section() {
        let w = 4.0;
        let mesh = tess(&[PaintCmd::Path {
            points: vec![Pos2::new(0.0, 10.0), Pos2::new(100.0, 10.0)],
            closed: false,
            width: w,
            color: Color::WHITE,
        }]);
        let prof = alpha_profile(&mesh, 50.0, 4.0, 16.0, 120);

        // Fully opaque in the middle…
        let mid = prof.iter().find(|(y, _)| (*y - 10.0).abs() < 0.06).unwrap();
        assert!(mid.1 > 0.99, "core is not solid: {mid:?}");
        // …and nothing outside the ramp, which straddles the nominal edge by
        // half a feather on purpose (that is what puts 50% coverage *on* the
        // edge rather than inside it).
        let outer = w * 0.5 + DEFAULT_FEATHERING * 0.5;
        for (y, a) in &prof {
            if (*y - 10.0).abs() > outer + 0.05 {
                assert!(*a < 0.01, "alpha {a} leaked past the ramp at y={y}");
            }
        }
        // Monotone up then down — a ramp, not a step.
        let up: Vec<f32> = prof
            .iter()
            .filter(|(y, _)| *y >= 10.0 - w * 0.5 - 0.1 && *y <= 10.0)
            .map(|(_, a)| *a)
            .collect();
        assert!(
            up.windows(2).all(|p| p[1] >= p[0] - 1e-3),
            "not monotone: {up:?}"
        );
        assert!(
            up.iter().any(|a| *a > 0.15 && *a < 0.85),
            "no partial-coverage samples — the edge is still hard: {up:?}"
        );
        // The 50% crossing sits on the nominal edge, within half a feather.
        let cross = prof
            .iter()
            .filter(|(y, _)| *y < 10.0)
            .min_by(|a, b| (a.1 - 0.5).abs().partial_cmp(&(b.1 - 0.5).abs()).unwrap())
            .unwrap();
        assert!(
            (cross.0 - (10.0 - w * 0.5)).abs() < 0.55,
            "50% coverage is not on the nominal edge: {cross:?}"
        );
    }

    /// Sub-pixel strokes fade instead of flickering: the ramp keeps a full
    /// feather of width and drops its peak alpha to `width / feathering`.
    #[test]
    fn thin_strokes_fade_rather_than_vanish() {
        for (w, expect) in [(0.25_f32, 0.25_f32), (0.5, 0.5), (1.0, 1.0)] {
            let mesh = tess(&[PaintCmd::Path {
                points: vec![Pos2::new(0.0, 10.0), Pos2::new(100.0, 10.0)],
                closed: false,
                width: w,
                color: Color::WHITE,
            }]);
            let peak = alpha_profile(&mesh, 50.0, 8.0, 12.0, 200)
                .into_iter()
                .map(|(_, a)| a)
                .fold(0.0f32, f32::max);
            assert!(
                (peak - expect).abs() < 0.06,
                "width {w}: peak alpha {peak}, expected ~{expect}"
            );
        }
    }

    /// A miter join keeps both flanks parallel to their segments, so the
    /// outer corner extends past the raw vertex by exactly 1/cos(theta/2)
    /// — no gap, no overlap spike.
    #[test]
    fn miter_join_neither_gaps_nor_spikes() {
        // A right angle: the miter should reach half_width * sqrt(2).
        let pts = [
            Pos2::new(0.0, 0.0),
            Pos2::new(50.0, 0.0),
            Pos2::new(50.0, 50.0),
        ];
        let n = path_normals(&pts, false);
        assert!(
            (n[1].length() - std::f32::consts::SQRT_2).abs() < 1e-3,
            "{:?}",
            n[1]
        );
        // Endpoints take their own segment normal, unit length.
        assert!((n[0].length() - 1.0).abs() < 1e-4);
        assert!((n[2].length() - 1.0).abs() < 1e-4);

        // A hairpin cannot produce an infinite spike.
        let hair = [
            Pos2::new(0.0, 0.0),
            Pos2::new(50.0, 0.0),
            Pos2::new(0.1, 0.0),
        ];
        let hn = path_normals(&hair, false);
        assert!(
            hn.iter()
                .all(|v| v.length().is_finite() && v.length() <= 4.001),
            "{hn:?}"
        );
    }

    /// Degenerate input must not produce NaN vertices or spurious geometry.
    #[test]
    fn degenerate_paths_are_safe() {
        let same = Pos2::new(5.0, 5.0);
        // Zero-length, single point, empty, and non-finite input.
        for pts in [
            vec![same, same, same],
            vec![same],
            vec![],
            vec![Pos2::new(f32::NAN, 0.0), Pos2::new(1.0, 1.0)],
        ] {
            let mesh = tess(&[PaintCmd::Path {
                points: pts.clone(),
                closed: false,
                width: 3.0,
                color: Color::WHITE,
            }]);
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.pos[0].is_finite() && v.pos[1].is_finite()),
                "NaN vertex from {pts:?}"
            );
        }
        // Collinear points keep a unit normal throughout.
        let line = [
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(20.0, 0.0),
        ];
        let n = path_normals(&line, false);
        assert!(n.iter().all(|v| (v.length() - 1.0).abs() < 1e-4), "{n:?}");
        // A closed 3-point path is still a triangle after dedup.
        let closed = dedup_path(
            &[same, Pos2::new(9.0, 5.0), Pos2::new(5.0, 9.0), same],
            true,
        );
        assert_eq!(closed.len(), 3);
    }

    /// A filled convex polygon keeps its size: the ramp straddles the
    /// outline rather than growing or shrinking the shape.
    #[test]
    fn convex_fill_feathers_around_its_outline() {
        let r = 20.0;
        let c = Pos2::new(50.0, 50.0);
        let pts: Vec<Pos2> = (0..32)
            .map(|i| {
                let t = i as f32 / 32.0 * std::f32::consts::TAU;
                Pos2::new(c.x + r * t.cos(), c.y + r * t.sin())
            })
            .collect();
        let mesh = tess(&[PaintCmd::ConvexPolygon {
            points: pts,
            color: Color::WHITE,
        }]);
        let prof = alpha_profile(&mesh, 50.0, 26.0, 34.0, 160);
        // Solid at the centre, clear well outside, ramped across the rim.
        let centre = alpha_profile(&mesh, 50.0, 49.0, 51.0, 4);
        assert!(
            centre.iter().all(|(_, a)| *a > 0.99),
            "centre not solid: {centre:?}"
        );
        assert!(prof.first().unwrap().1 < 0.01, "alpha outside the disc");
        assert!(
            prof.last().unwrap().1 > 0.99,
            "not solid just inside the rim"
        );
        assert!(
            prof.iter().any(|(_, a)| *a > 0.15 && *a < 0.85),
            "rim is not feathered"
        );
        // The 50% crossing is within half a feather of the true edge.
        let cross = prof
            .iter()
            .min_by(|a, b| (a.1 - 0.5).abs().partial_cmp(&(b.1 - 0.5).abs()).unwrap())
            .unwrap();
        assert!((cross.0 - 30.0).abs() < 0.6, "rim moved: {cross:?}");
    }

    /// Feathering off reproduces the old hard-edged output exactly.
    #[test]
    fn feathering_can_be_disabled() {
        let mut t = Tessellator::new().with_feathering(0.0);
        let mut m = Mesh::default();
        t.reset(&mut m);
        t.tessellate(
            &[PaintCmd::Path {
                points: vec![Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)],
                closed: false,
                width: 3.0,
                color: Color::WHITE,
            }],
            &mut m,
        );
        assert_eq!(m.vertices.len(), 4, "one quad, no ramps");
        assert!(m.vertices.iter().all(|v| v.color[3] == 1.0));
        assert_eq!(t.feathering(), 0.0);
    }

    /// Feathering costs vertices; this pins how many so a future change to the
    /// strip layout cannot quietly triple the mesh.
    ///
    /// A stroked path goes from 4 verts/segment (independent quads) to
    /// 4 verts/*point* in one strip — so a long path is roughly 1x, and only
    /// short paths pay the join overhead. A convex fill goes from N to 2N.
    #[test]
    fn feathering_vertex_cost_is_bounded() {
        // A scene shaped like the node-graph canvas: a sampled wire, some
        // pin discs, and a couple of short chrome strokes.
        let wire: Vec<Pos2> = (0..=64)
            .map(|i| {
                let t = i as f32 / 64.0;
                Pos2::new(t * 400.0, 100.0 + (t * 3.0).sin() * 40.0)
            })
            .collect();
        let disc = |c: Pos2, r: f32| -> Vec<Pos2> {
            (0..20)
                .map(|i| {
                    let t = i as f32 / 20.0 * std::f32::consts::TAU;
                    Pos2::new(c.x + r * t.cos(), c.y + r * t.sin())
                })
                .collect()
        };
        let mut cmds = vec![PaintCmd::Path {
            points: wire,
            closed: false,
            width: 1.9,
            color: Color::WHITE,
        }];
        for i in 0..6 {
            cmds.push(PaintCmd::ConvexPolygon {
                points: disc(Pos2::new(20.0 + i as f32 * 60.0, 100.0), 4.5),
                color: Color::WHITE,
            });
            cmds.push(PaintCmd::Path {
                points: disc(Pos2::new(20.0 + i as f32 * 60.0, 160.0), 4.5),
                closed: true,
                width: 1.5,
                color: Color::WHITE,
            });
        }

        let count = |feather: f32| {
            let mut t = Tessellator::new().with_feathering(feather);
            let mut m = Mesh::default();
            t.reset(&mut m);
            t.tessellate(&cmds, &mut m);
            (m.vertices.len(), m.indices.len())
        };
        let (hard_v, hard_i) = count(0.0);
        let (soft_v, soft_i) = count(DEFAULT_FEATHERING);
        let vr = soft_v as f32 / hard_v as f32;
        let ir = soft_i as f32 / hard_i as f32;
        // Recorded so the numbers are visible in the test output.
        println!("verts {hard_v} -> {soft_v} ({vr:.2}x), indices {hard_i} -> {soft_i} ({ir:.2}x)");
        assert!(
            (1.0..=3.0).contains(&vr),
            "feathered vertex count moved out of its expected band: {vr:.2}x"
        );
        assert!(ir <= 3.5, "index count grew more than expected: {ir:.2}x");
    }

    #[test]
    fn mesh_cmd_copies_indexed_geometry_with_base_offset() {
        let mut tess = Tessellator::new();
        let mut mesh = Mesh::default();
        tess.reset(&mut mesh);
        let cmds = [
            PaintCmd::Rect {
                rect: Rect::from_min_size(Pos2::new(0.0, 0.0), crate::math::Vec2::new(1.0, 1.0)),
                color: Color::WHITE,
            },
            PaintCmd::Mesh {
                vertices: vec![
                    (Pos2::new(0.0, 0.0), Color::rgba(1.0, 0.0, 0.0, 1.0)),
                    (Pos2::new(2.0, 0.0), Color::rgba(0.0, 1.0, 0.0, 0.5)),
                    (Pos2::new(0.0, 2.0), Color::rgba(0.0, 0.0, 1.0, 1.0)),
                ],
                indices: vec![0, 1, 2],
            },
        ];
        tess.tessellate(&cmds, &mut mesh);
        // 4 quad vertices, then the 3 mesh vertices; indices rebased past the quad.
        assert_eq!(mesh.vertices.len(), 7);
        assert_eq!(&mesh.indices[6..], &[4, 5, 6]);
        assert_eq!(mesh.vertices[5].color, [0.0, 1.0, 0.0, 0.5]);
        assert_eq!(mesh.vertices[4].params[3], MODE_SOLID);
    }
}
