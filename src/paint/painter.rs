//! A low-level drawing handle.
//!
//! [`Painter`] is the escape hatch beneath the widget layer: it borrows a
//! paint buffer and the [`TextRenderer`] and exposes direct primitives —
//! lines, polylines, rects, circles, convex polygons, bezier curves, and
//! text-at-a-point. Custom overlays (flamegraphs, gizmos, plots, profiler
//! drawings) build on this rather than reaching into [`PaintCmd`] by hand.
//!
//! Get one from [`Ui::painter`](crate::context::Ui::painter). Everything it
//! emits lands in the buffer it was handed, so it inherits whatever clip
//! region is active in that buffer at tessellation time.

use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::text::TextRenderer;

/// Borrowed drawing handle over a [`PaintCmd`] buffer plus the text renderer.
/// Cheap to create; create one per drawing burst.
pub struct Painter<'a> {
    paint: &'a mut Vec<PaintCmd>,
    text: &'a mut TextRenderer,
    clip: Rect,
}

impl<'a> Painter<'a> {
    /// Construct a painter over `paint`/`text`, advertising `clip` as the
    /// region callers should stay within. The painter does *not* itself push
    /// a clip command — clipping is governed by the buffer's existing
    /// `PushClip`/`PopClip` state.
    pub fn new(paint: &'a mut Vec<PaintCmd>, text: &'a mut TextRenderer, clip: Rect) -> Self {
        Self { paint, text, clip }
    }

    /// The region this painter advertises for layout/culling.
    #[inline]
    pub fn clip_rect(&self) -> Rect {
        self.clip
    }

    /// Mutable access to the underlying command buffer for advanced callers.
    #[inline]
    pub fn paint_mut(&mut self) -> &mut Vec<PaintCmd> {
        self.paint
    }

    // ── strokes ──────────────────────────────────────────────────────────

    /// A single straight segment with square caps.
    pub fn line_segment(&mut self, from: Pos2, to: Pos2, width: f32, color: Color) {
        self.paint.push(PaintCmd::Line { from, to, width, color });
    }

    /// A connected open polyline through `points`.
    ///
    /// Emitted as one path, not a run of segments: the tessellator miters the
    /// joints and feathers the whole outline in one strip, which is both
    /// smoother and cheaper than overlapping quads.
    pub fn polyline(&mut self, points: &[Pos2], width: f32, color: Color) {
        if points.len() < 2 || width <= 0.0 {
            return;
        }
        self.paint.push(PaintCmd::Path {
            points: points.to_vec(),
            closed: false,
            width,
            color,
        });
    }

    /// A closed polyline (the last point joins back to the first).
    pub fn polygon_stroke(&mut self, points: &[Pos2], width: f32, color: Color) {
        if points.len() < 2 || width <= 0.0 {
            return;
        }
        self.paint.push(PaintCmd::Path {
            points: points.to_vec(),
            closed: true,
            width,
            color,
        });
    }

    // ── rects ────────────────────────────────────────────────────────────

    pub fn rect_filled(
        &mut self,
        rect: Rect,
        rounding: impl Into<Rounding>,
        color: Color,
    ) {
        self.paint
            .push(PaintCmd::Rrect(RectShape::fill(rect, rounding, color)));
    }

    pub fn rect_stroke(
        &mut self,
        rect: Rect,
        rounding: impl Into<Rounding>,
        width: f32,
        color: Color,
    ) {
        // `fill` must be `None`: `Some(TRANSPARENT)` takes the shader's glass
        // path, which composites the opaque backdrop over the interior instead
        // of leaving it transparent.
        let mut shape = RectShape::fill(rect, rounding, Color::TRANSPARENT).with_stroke(width, color);
        shape.fill = None;
        self.paint.push(PaintCmd::Rrect(shape));
    }

    // ── filled shapes ────────────────────────────────────────────────────

    /// A solid triangle.
    pub fn triangle(&mut self, a: Pos2, b: Pos2, c: Pos2, color: Color) {
        self.paint.push(PaintCmd::ConvexPolygon {
            points: vec![a, b, c],
            color,
        });
    }

    /// A solid convex polygon (fan-triangulated from the first vertex).
    pub fn convex_polygon_filled(&mut self, points: Vec<Pos2>, color: Color) {
        self.paint.push(PaintCmd::ConvexPolygon { points, color });
    }

    /// Pre-triangulated indexed geometry with per-vertex colors (drawn
    /// flat, no texture). For externally tessellated shapes that are
    /// neither convex nor single-colored.
    pub fn mesh(&mut self, vertices: Vec<(Pos2, Color)>, indices: Vec<u32>) {
        self.paint.push(PaintCmd::Mesh { vertices, indices });
    }

    /// A filled circle approximated by an n-gon whose vertex count scales
    /// with `radius` so it stays smooth at any size.
    pub fn circle_filled(&mut self, center: Pos2, radius: f32, color: Color) {
        let pts = circle_points(center, radius);
        self.convex_polygon_filled(pts, color);
    }

    /// A circle outline.
    pub fn circle_stroke(&mut self, center: Pos2, radius: f32, width: f32, color: Color) {
        let pts = circle_points(center, radius);
        self.polygon_stroke(&pts, width, color);
    }

    // ── bezier curves ────────────────────────────────────────────────────

    /// Stroke a cubic bezier through its four control points.
    pub fn bezier_cubic(
        &mut self,
        p0: Pos2,
        p1: Pos2,
        p2: Pos2,
        p3: Pos2,
        width: f32,
        color: Color,
    ) {
        let pts = sample_cubic(p0, p1, p2, p3);
        self.polyline(&pts, width, color);
    }

    /// Stroke a quadratic bezier through its three control points.
    pub fn bezier_quadratic(
        &mut self,
        p0: Pos2,
        p1: Pos2,
        p2: Pos2,
        width: f32,
        color: Color,
    ) {
        // Promote to a cubic so we share one sampler.
        let c1 = lerp(p0, p1, 2.0 / 3.0);
        let c2 = lerp(p2, p1, 2.0 / 3.0);
        self.bezier_cubic(p0, c1, c2, p2, width, color);
    }

    // ── text ─────────────────────────────────────────────────────────────

    /// Draw `text` at `pos` (top-left), returning the laid-out size. Pass
    /// `max_width` to wrap.
    pub fn text(
        &mut self,
        pos: Pos2,
        text: &str,
        size_px: f32,
        color: Color,
        max_width: Option<f32>,
    ) -> Vec2 {
        let size = self.text.measure(text, size_px, max_width);
        self.text
            .add_text(self.paint, text, size_px, pos, color, max_width);
        size
    }

    /// Measure `text` without drawing it.
    pub fn measure_text(&mut self, text: &str, size_px: f32, max_width: Option<f32>) -> Vec2 {
        self.text.measure(text, size_px, max_width)
    }

    /// [`Painter::text`] with an explicit font family.
    pub fn text_family(
        &mut self,
        pos: Pos2,
        text: &str,
        size_px: f32,
        color: Color,
        max_width: Option<f32>,
        family: crate::text::FontFamily,
    ) -> Vec2 {
        let size = self.text.measure_family(text, size_px, max_width, family);
        self.text
            .add_text_family(self.paint, text, size_px, pos, color, max_width, family);
        size
    }

    /// [`Painter::measure_text`] with an explicit font family.
    pub fn measure_text_family(
        &mut self,
        text: &str,
        size_px: f32,
        max_width: Option<f32>,
        family: crate::text::FontFamily,
    ) -> Vec2 {
        self.text.measure_family(text, size_px, max_width, family)
    }
}

/// Segment count for a circle of a given pixel radius.
///
/// The floor is what matters: small circles are the common case in editor
/// chrome (pin dots, radio buttons, swatch dots, handles) and the old floor
/// of 8 drew them as visible octagons — at r=4.5 the previous formula asked
/// for 3 segments and got the floor. 20 is the point where a small disc reads
/// as round; above that the radius term takes over, capped so a big circle
/// stays cheap. Tens of extra vertices per shape, at most.
fn arc_segments(radius: f32) -> usize {
    (radius * 3.0).ceil().clamp(20.0, 64.0) as usize
}

fn circle_points(center: Pos2, radius: f32) -> Vec<Pos2> {
    let n = arc_segments(radius);
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            Pos2::new(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect()
}

fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    Pos2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn sample_cubic(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) -> Vec<Pos2> {
    // Crude flatness proxy: total control-polygon length sets the segment
    // count, so longer/wigglier curves get more samples. Callers pass control
    // points in the space they draw in, so `span` is already screen-space and
    // the density tracks zoom for free.
    //
    // The rate targets ~4px per segment; the *cap* is what used to bite — at
    // 64 segments a long curve (a zoomed-in graph wire spanning 1500px) drew
    // 23px chords and read as a chain of straight bars. The floor rose for
    // the same reason a circle's did: a short, tightly-curved bezier at 8
    // segments is visibly faceted.
    let span = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
    let n = (span * 0.25).clamp(16.0, 256.0) as usize;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let w0 = u * u * u;
            let w1 = 3.0 * u * u * t;
            let w2 = 3.0 * u * t * t;
            let w3 = t * t * t;
            Pos2::new(
                w0 * p0.x + w1 * p1.x + w2 * p2.x + w3 * p3.x,
                w0 * p0.y + w1 * p1.y + w2 * p2.y + w3 * p3.y,
            )
        })
        .collect()
}
