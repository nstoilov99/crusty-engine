//! Pan/zoom canvas (Phase 22): a world-space content region for node-graph
//! style editors. The caller owns the view transform (`CanvasView`) and
//! passes it in mutably — the canvas applies pan/zoom input to it, clips its
//! body, and hands the body a `CanvasScope` for world↔screen conversion and
//! world-space hit testing. The body draws its content (nodes, wires) through
//! the regular `Painter` in screen coordinates obtained from the scope.
//!
//! View state deliberately does *not* live in GUI `Memory`: memory is
//! per-context (a torn-off dock window is a different context) and is not
//! serialized, while node-editor view state belongs to the app's document
//! state. Only the transient middle-drag origin is kept in `Memory`.

use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::input::MouseButton;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::PaintCmd;
use crate::response::Response;

/// Caller-owned pan/zoom state. `pan` is the world-space point rendered at
/// the canvas rect's top-left corner; `zoom` is screen pixels per world unit.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CanvasView {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0 }
    }
}

/// Discrete zoom steps used to quantize label font sizes
/// ([`CanvasScope::label_size`]). Text can only "zoom" by re-shaping at a new
/// pixel size, and every distinct size is a shape-cache + glyph-atlas entry —
/// snapping labels to a small ladder keeps smooth geometric zoom from
/// churning both. Geometry zooms continuously; only text snaps.
const TEXT_ZOOM_LADDER: [f32; 7] = [0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0];

/// Labels smaller than this many pixels are reported as hidden.
const MIN_LABEL_PX: f32 = 7.0;

/// Middle-drag pan state, kept in typed `Memory` while the drag is live.
#[derive(Clone, Copy)]
struct PanDrag {
    last: Pos2,
}

/// Snapshot of the view mapped onto this frame's canvas rect. Handed to the
/// body for coordinate conversion and world-space hit tests.
#[derive(Clone, Copy, Debug)]
pub struct CanvasScope {
    rect: Rect,
    pan: Vec2,
    zoom: f32,
}

impl CanvasScope {
    /// The canvas widget's screen rect.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn world_to_screen(&self, w: Pos2) -> Pos2 {
        self.rect.min + (w.to_vec2() - self.pan) * self.zoom
    }

    pub fn screen_to_world(&self, s: Pos2) -> Pos2 {
        Pos2::new(0.0, 0.0) + self.pan + (s - self.rect.min) / self.zoom
    }

    pub fn world_rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.world_to_screen(r.min), self.world_to_screen(r.max))
    }

    /// The world-space region currently visible — cull against this before
    /// drawing/interacting per element (there is no library-side culling).
    pub fn visible_world_rect(&self) -> Rect {
        Rect::from_min_max(
            self.screen_to_world(self.rect.min),
            self.screen_to_world(self.rect.max),
        )
    }

    /// Hover/press/click for a world-space rect: converts through the
    /// transform and runs the standard [`Ui::interact`] protocol (which
    /// already respects the canvas clip). Primary button only, like every
    /// `interact` — the canvas itself owns middle-drag.
    pub fn interact(&self, ui: &mut Ui, id: Id, world_rect: Rect) -> Response {
        ui.interact(id, self.world_rect_to_screen(world_rect))
    }

    /// Pointer position in world space, if the pointer is inside the canvas.
    pub fn pointer_world(&self, ui: &Ui) -> Option<Pos2> {
        ui.ctx()
            .input
            .pointer_pos
            .filter(|p| self.rect.contains(*p))
            .map(|p| self.screen_to_world(p))
    }

    /// Font size for a label whose base size (at zoom 1.0) is `base_px`,
    /// quantized to the text zoom ladder. `None` = too small, hide the label.
    pub fn label_size(&self, base_px: f32) -> Option<f32> {
        let step = TEXT_ZOOM_LADDER
            .iter()
            .copied()
            .min_by(|a, b| {
                (a - self.zoom)
                    .abs()
                    .partial_cmp(&(b - self.zoom).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(1.0);
        let px = base_px * step;
        (px >= MIN_LABEL_PX).then_some(px)
    }
}

/// What [`Canvas::show`] reports back besides the body's return value.
pub struct CanvasOutput<R> {
    pub inner: R,
    pub rect: Rect,
    pub hovered: bool,
    /// Pointer position in world space while hovering the canvas.
    pub pointer_world: Option<Pos2>,
    /// The view was panned or zoomed this frame (mark app state dirty-for-
    /// persistence if you save view state; document data is unaffected).
    pub view_changed: bool,
}

/// The pan/zoom canvas widget. Input: middle-drag pans, scroll wheel pans
/// (both axes), Ctrl+scroll zooms about the pointer.
pub struct Canvas {
    size: Option<Vec2>,
    min_zoom: f32,
    max_zoom: f32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    pub fn new() -> Self {
        Self { size: None, min_zoom: 0.25, max_zoom: 2.5 }
    }

    /// Fixed size instead of filling the available rect.
    pub fn size(mut self, v: Vec2) -> Self {
        self.size = Some(v);
        self
    }

    pub fn zoom_range(mut self, min: f32, max: f32) -> Self {
        self.min_zoom = min.max(0.01);
        self.max_zoom = max.max(self.min_zoom);
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        view: &mut CanvasView,
        body: impl FnOnce(&mut Ui, &CanvasScope) -> R,
    ) -> CanvasOutput<R> {
        let id = ui.alloc_id("canvas");
        let size = self.size.unwrap_or_else(|| {
            let a = ui.available_size();
            Vec2::new(a.x.max(1.0), a.y.max(1.0))
        });
        let rect = ui.allocate(size);
        let hovered = ui.contains_pointer(rect);
        let view_before = *view;

        self.apply_input(ui, id, rect, hovered, view);
        view.zoom = view.zoom.clamp(self.min_zoom, self.max_zoom);

        let scope = CanvasScope { rect, pan: view.pan, zoom: view.zoom };

        // Clip body paint to the widget rect (a child Ui inherits the parent
        // clip only) and give the child the same rect as its hit-test clip.
        let clip = rect.intersect(ui.clip_rect());
        ui.ctx_mut().paint.push(PaintCmd::PushClip(clip));
        let inner = {
            let opts = UiOptions { padding: Vec2::ZERO, spacing: 0.0 };
            let parent_area = ui.area();
            let Ui { ctx, text, .. } = ui;
            let mut child = Ui::with_root(
                ctx,
                text,
                rect,
                Direction::TopDown,
                opts,
                id.with("body"),
            );
            child.set_area(parent_area);
            child.set_clip(clip);
            body(&mut child, &scope)
        };
        ui.ctx_mut().paint.push(PaintCmd::PopClip);

        // Standard widget-memory upkeep.
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;

        CanvasOutput {
            inner,
            rect,
            hovered,
            pointer_world: scope.pointer_world(ui),
            view_changed: *view != view_before,
        }
    }

    fn apply_input(
        &self,
        ui: &mut Ui,
        id: Id,
        rect: Rect,
        hovered: bool,
        view: &mut CanvasView,
    ) {
        let pointer = ui.ctx().input.pointer_pos;

        // Middle-drag pan. `Ui::interact` is primary-only, so this is built
        // on the raw input state; the drag origin lives in typed `Memory`
        // (read with `data_get_mut` so GC sees it — see `dnd_drag_source`).
        let middle_down = ui.ctx().input.middle_down;
        let middle_pressed = ui
            .ctx()
            .input
            .button_presses
            .contains(&MouseButton::Middle);
        if let Some(p) = pointer.filter(|_| middle_pressed && hovered) {
            ui.ctx_mut().memory.data_insert(id, PanDrag { last: p });
        }
        if middle_down {
            if let (Some(p), Some(drag)) = (
                pointer,
                ui.ctx_mut().memory.data_get_mut::<PanDrag>(id),
            ) {
                let delta = p - drag.last;
                drag.last = p;
                view.pan -= delta / view.zoom;
            }
        } else {
            ui.ctx_mut().memory.data_remove::<PanDrag>(id);
        }

        // Wheel: Ctrl+scroll zooms about the pointer, plain scroll pans
        // (same direction convention as `ScrollArea`).
        let scroll = ui.ctx().input.scroll_delta;
        if hovered && (scroll.x != 0.0 || scroll.y != 0.0) {
            if ui.ctx().input.modifiers.contains(crate::input::Modifiers::CTRL) {
                if let Some(p) = pointer {
                    let world_at_pointer =
                        Pos2::new(0.0, 0.0) + view.pan + (p - rect.min) / view.zoom;
                    view.zoom = (view.zoom * (scroll.y * 0.12).exp())
                        .clamp(self.min_zoom, self.max_zoom);
                    view.pan =
                        world_at_pointer.to_vec2() - (p - rect.min) / view.zoom;
                }
            } else {
                view.pan -= scroll * 30.0 / view.zoom;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(pan: Vec2, zoom: f32) -> CanvasScope {
        CanvasScope {
            rect: Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(400.0, 300.0)),
            pan,
            zoom,
        }
    }

    #[test]
    fn world_screen_round_trip() {
        let s = scope(Vec2::new(-20.0, 35.0), 1.7);
        let w = Pos2::new(12.5, -80.0);
        let back = s.screen_to_world(s.world_to_screen(w));
        assert!((back.x - w.x).abs() < 1e-3 && (back.y - w.y).abs() < 1e-3);
    }

    #[test]
    fn pan_is_world_at_top_left() {
        let s = scope(Vec2::new(7.0, -3.0), 2.0);
        let tl = s.screen_to_world(s.rect().min);
        assert_eq!((tl.x, tl.y), (7.0, -3.0));
    }

    #[test]
    fn zoom_about_pointer_keeps_point_fixed() {
        // Reproduce the zoom step from `apply_input` and assert the world
        // point under the pointer stays put across the zoom change.
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, 200.0));
        let mut view = CanvasView { pan: Vec2::new(10.0, 10.0), zoom: 1.0 };
        let p = Pos2::new(150.0, 60.0);
        let before = CanvasScope { rect, pan: view.pan, zoom: view.zoom }
            .screen_to_world(p);
        let world_at_pointer =
            Pos2::new(0.0, 0.0) + view.pan + (p - rect.min) / view.zoom;
        view.zoom = (view.zoom * 0.6_f32.exp()).clamp(0.25, 2.5);
        view.pan = world_at_pointer.to_vec2() - (p - rect.min) / view.zoom;
        let after = CanvasScope { rect, pan: view.pan, zoom: view.zoom }
            .screen_to_world(p);
        assert!((after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3);
    }

    #[test]
    fn label_size_quantizes_and_hides() {
        let s = scope(Vec2::ZERO, 1.32);
        // 1.32 snaps to ladder step 1.5.
        assert_eq!(s.label_size(12.0), Some(18.0));
        // Deep zoom-out hides small labels: 0.3 snaps to 0.25 → 3px < 7px.
        let far = scope(Vec2::ZERO, 0.3);
        assert_eq!(far.label_size(12.0), None);
    }

    #[test]
    fn visible_world_rect_matches_rect_extent() {
        let s = scope(Vec2::new(0.0, 0.0), 2.0);
        let v = s.visible_world_rect();
        assert_eq!(v.width(), s.rect().width() / 2.0);
        assert_eq!(v.height(), s.rect().height() / 2.0);
    }
}
