//! Cursor-based immediate-mode UI handle.
//!
//! A [`Ui`] borrows the [`Context`](super::Context) and a
//! [`TextRenderer`](crate::text::TextRenderer) for the duration of a
//! single layout pass, and tracks a "cursor" that advances as widgets
//! allocate space inside the available rect.

use crate::context::Context;
use crate::id::Id;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::text::TextRenderer;
use std::any::TypeId;

/// Pointer travel (px) before a press inside a drag source turns into an
/// actual drag, so a click doesn't accidentally start dragging.
const DRAG_THRESHOLD: f32 = 4.0;

/// Direction the cursor advances when widgets allocate space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    TopDown,
    LeftToRight,
}

/// Position of a group within its available cross/main extent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Align {
    /// Flush to the start (left / top).
    Min,
    /// Centered in the available extent.
    Center,
    /// Flush to the end (right / bottom).
    Max,
}

#[derive(Clone, Copy, Debug)]
pub struct UiOptions {
    /// Padding inside the available rect before the cursor starts.
    pub padding: Vec2,
    /// Space between consecutive widgets in the layout direction.
    pub spacing: f32,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            padding: Vec2::splat(12.0),
            spacing: 8.0,
        }
    }
}

pub struct Ui<'a> {
    pub(crate) ctx: &'a mut Context,
    pub(crate) text: &'a mut TextRenderer,
    available: Rect,
    cursor: Pos2,
    direction: Direction,
    spacing: f32,
    /// Salt for child Ids — every widget gets `parent_id.with(index)`.
    next_index: u64,
    /// Tracks the bounding box of all child widgets so callers can know
    /// how much space the Ui actually used.
    used: Rect,
    /// Id namespace prefix. Every widget Id is derived from this, so
    /// otherwise-identical labels in different Uis (e.g. two docked
    /// panels) get distinct Ids and don't collide in Memory.
    root_id: Id,
    /// Floating area this Ui belongs to (`None` = root UI). Drives
    /// occlusion-aware hit testing in [`contains_pointer`](Self::contains_pointer).
    area: Option<Id>,
    /// Hit-test bound. Widget rects are intersected with this before testing
    /// the pointer, so content that overflows its container (e.g. a dock
    /// panel) can't be clicked outside the container's bounds. Defaults to
    /// the Ui's full rect; scroll viewports and clipped containers narrow it.
    clip: Rect,
}

impl<'a> Ui<'a> {
    pub fn new(
        ctx: &'a mut Context,
        text: &'a mut TextRenderer,
        rect: Rect,
        direction: Direction,
        opts: UiOptions,
    ) -> Self {
        Self::with_root(ctx, text, rect, direction, opts, Id::ROOT.with("ui"))
    }

    pub fn with_root(
        ctx: &'a mut Context,
        text: &'a mut TextRenderer,
        rect: Rect,
        direction: Direction,
        opts: UiOptions,
        root_id: Id,
    ) -> Self {
        let available = rect.shrink2(opts.padding);
        let cursor = available.min;
        Self {
            ctx,
            text,
            available,
            cursor,
            direction,
            spacing: opts.spacing,
            next_index: 0,
            used: Rect::from_min_size(cursor, Vec2::ZERO),
            root_id,
            area: None,
            clip: rect,
        }
    }

    /// Bind this Ui (and the widgets inside it) to a floating area so
    /// hit testing respects z-order. Containers like `Area`, `Window`
    /// and popup bodies call this right after construction.
    pub fn set_area(&mut self, area: Option<Id>) {
        self.area = area;
    }

    /// Floating area this Ui belongs to (`None` = root UI).
    pub fn area(&self) -> Option<Id> {
        self.area
    }

    /// The current hit-test bound. Widget rects are intersected with this in
    /// [`contains_pointer`](Self::contains_pointer).
    pub fn clip_rect(&self) -> Rect {
        self.clip
    }

    /// Narrow (or replace) the hit-test bound. Containers that scissor their
    /// content — scroll viewports, clipped panels — call this so overflowing
    /// widgets can't be clicked outside the visible region.
    pub fn set_clip(&mut self, clip: Rect) {
        self.clip = clip;
    }

    /// Occlusion-aware pointer test: true when the pointer is inside
    /// `rect` and no floating area above this Ui's layer (or a modal)
    /// owns the pointer. Widgets use this instead of raw
    /// `input.pointer_in`.
    pub fn contains_pointer(&self, rect: Rect) -> bool {
        self.ctx.pointer_over(self.area, rect.intersect(self.clip))
    }

    pub fn ctx(&self) -> &Context {
        self.ctx
    }
    pub fn ctx_mut(&mut self) -> &mut Context {
        self.ctx
    }
    pub fn text(&self) -> &TextRenderer {
        self.text
    }
    pub fn text_mut(&mut self) -> &mut TextRenderer {
        self.text
    }
    pub fn available(&self) -> Rect {
        self.available
    }
    /// Space remaining from the cursor to the far edge of the available
    /// rect, clamped to non-negative. Widgets negotiate their size against
    /// this (e.g. a slider fills the leftover width, text wraps to it).
    pub fn available_size(&self) -> Vec2 {
        Vec2::new(
            (self.available.max.x - self.cursor.x).max(0.0),
            (self.available.max.y - self.cursor.y).max(0.0),
        )
    }
    pub fn cursor(&self) -> Pos2 {
        self.cursor
    }
    /// Snapshot of the active style. Widgets typically take this once at
    /// the top of their `show` and then read fields by value, which keeps
    /// the borrow on `ctx` short and avoids fighting the borrow checker
    /// when other `ui.*` methods need `&mut`.
    pub fn style(&self) -> crate::style::Style {
        self.ctx.style
    }

    /// A low-level [`Painter`](crate::paint::Painter) over this Ui's main
    /// paint buffer, advertising the remaining available rect as its clip.
    /// Use it for custom drawing (flamegraphs, gizmos, plots) that the
    /// widget set doesn't cover. Does not advance the cursor — pair it with
    /// [`allocate`](Self::allocate) when the drawing should occupy layout
    /// space.
    pub fn painter(&mut self) -> crate::paint::Painter<'_> {
        let clip = self.clip;
        crate::paint::Painter::new(&mut self.ctx.paint, &mut *self.text, clip)
    }

    /// Like [`painter`](Self::painter) but targets the overlay buffer, so the
    /// drawing renders above the regular UI (tooltips, drag previews).
    pub fn overlay_painter(&mut self) -> crate::paint::Painter<'_> {
        let clip = self.clip;
        crate::paint::Painter::new(&mut self.ctx.overlay_paint, &mut *self.text, clip)
    }

    /// Raw-rect interaction for custom-drawn widgets (tree rows, plot
    /// regions): hover / press / click against `rect` with the standard
    /// active-widget protocol, without allocating layout space or painting
    /// anything. Combine with [`Response::double_clicked`] for
    /// double-clicks and [`dnd_drag_source`](Self::dnd_drag_source) (same
    /// `id`) for drags. A release that ends an in-flight drag does not
    /// count as a click.
    pub fn interact(&mut self, id: Id, rect: Rect) -> crate::response::Response {
        let hovered = self.contains_pointer(rect);
        let pointer_pressed = self.ctx.input.pointer_pressed;
        let pointer_released = self.ctx.input.pointer_released;
        let pointer_down = self.ctx.input.pointer_down;
        if pointer_pressed && hovered {
            self.ctx.memory.active_widget = Some(id);
        }
        let is_active = self.ctx.memory.active_widget == Some(id);
        let pressed = is_active && pointer_down;
        let clicked =
            pointer_released && is_active && hovered && !self.ctx.dnd.is_dragging();

        let dt = self.ctx.dt();
        let style = self.ctx.style;
        let mem = self.ctx.memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim = crate::animation::approach_half_life(
            mem.hover_anim,
            if hovered { 1.0 } else { 0.0 },
            dt,
            style.motion.hover_half_life,
        );
        mem.press_anim = crate::animation::approach_half_life(
            mem.press_anim,
            if pressed { 1.0 } else { 0.0 },
            dt,
            style.motion.press_half_life,
        );
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        crate::response::Response {
            id,
            rect,
            hovered,
            pressed,
            clicked,
            hover_anim,
            press_anim,
        }
    }

    /// Drag source. When the user presses inside `rect` and moves past a
    /// small threshold, begins carrying a type-erased `payload` (built
    /// lazily so it's only constructed once a real drag starts) labelled
    /// `label`. A translucent ghost follows the pointer until release.
    /// Returns `true` while this source's drag is active so the caller can
    /// dim the original.
    pub fn dnd_drag_source<T: 'static>(
        &mut self,
        id: Id,
        rect: Rect,
        label: impl Into<String>,
        payload: impl FnOnce() -> T,
    ) -> bool {
        if self.ctx.input.pointer_pressed && self.contains_pointer(rect) {
            if let Some(p) = self.ctx.input.pointer_pos {
                self.ctx.memory.active_widget = Some(id);
                self.ctx.memory.data_insert::<Pos2>(id, p);
            }
        }

        if self.ctx.memory.active_widget != Some(id) {
            return false;
        }

        // Already carrying this payload — keep the ghost alive.
        if self.ctx.dnd.is_dragging() {
            if self.ctx.input.pointer_down {
                self.render_dnd_ghost();
                return true;
            }
            return false;
        }

        // Pressed but not yet activated: arm once the pointer travels far
        // enough from the press origin. Read the origin with `data_get_mut`
        // so the entry is marked alive each frame — `data_get` doesn't, and
        // memory GC would evict the origin after a few frames, silently
        // killing any drag slower than a quick flick.
        if self.ctx.input.pointer_down {
            if let (Some(p), Some(start)) = (
                self.ctx.input.pointer_pos,
                self.ctx.memory.data_get_mut::<Pos2>(id).map(|s| *s),
            ) {
                let dx = p.x - start.x;
                let dy = p.y - start.y;
                if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
                    let value = payload();
                    self.ctx.dnd.begin(
                        Box::new(value),
                        TypeId::of::<T>(),
                        label.into(),
                    );
                    self.render_dnd_ghost();
                    return true;
                }
            }
        }
        false
    }

    /// Drop target. If a payload of type `T` is hovering `rect` and the
    /// pointer is released this frame, takes it and returns the value,
    /// ending the drag.
    pub fn dnd_drop_target<T: 'static>(&mut self, rect: Rect) -> Option<T> {
        if self.ctx.input.pointer_released
            && self.ctx.dnd.holds::<T>()
            && self.contains_pointer(rect)
        {
            self.ctx.dnd.take::<T>()
        } else {
            None
        }
    }

    /// True when a payload of type `T` is in flight and currently over
    /// `rect`. Use to highlight compatible slots mid-drag.
    pub fn dnd_hovering<T: 'static>(&self, rect: Rect) -> bool {
        self.ctx.dnd.is_dragging()
            && self.ctx.dnd.holds::<T>()
            && self.contains_pointer(rect)
    }

    /// Draw a faint labelled chip at the pointer into the overlay layer so
    /// the drag preview floats above everything else.
    fn render_dnd_ghost(&mut self) {
        let Some(pos) = self.ctx.input.pointer_pos else {
            return;
        };
        let Some(label) = self.ctx.dnd.label().map(str::to_owned) else {
            return;
        };
        let style = self.ctx.style;
        let label_size = self.text.measure(&label, style.fonts.body, None);
        let pad = Vec2::new(10.0, 6.0);
        let size = Vec2::new(label_size.x + pad.x * 2.0, label_size.y + pad.y * 2.0);
        let origin = Pos2::new(pos.x + 8.0, pos.y - size.y * 0.5);
        let rect = Rect::from_min_size(origin, size);

        let mut fill = style.palette.surface;
        fill.a = 0.7;
        let mut stroke = style.palette.accent;
        stroke.a = 0.85;
        let shape =
            RectShape::fill(rect, style.rounding.small, fill).with_stroke(1.0, stroke);
        self.ctx.overlay_paint.push(PaintCmd::Rrect(shape));

        let text_pos = Pos2::new(rect.min.x + pad.x, rect.min.y + pad.y);
        self.text.add_text(
            &mut self.ctx.overlay_paint,
            &label,
            style.fonts.body,
            text_pos,
            style.palette.text.with_alpha(0.9),
            None,
        );
    }

    /// Allocate a fresh widget Id derived from the Ui's index counter and
    /// an optional source hash (e.g. the widget's text). Same combination
    /// of (parent, source, index) on consecutive frames yields the same
    /// Id, which is what makes the persistent [`Memory`](super::Memory)
    /// work across frames.
    pub fn alloc_id<S: std::hash::Hash>(&mut self, source: S) -> Id {
        let id = self.root_id.with(&source).with(self.next_index);
        self.next_index = self.next_index.wrapping_add(1);
        id
    }

    /// Reserve a rect of the given `size` at the current cursor and
    /// advance the cursor by that amount plus spacing.
    pub fn allocate(&mut self, size: Vec2) -> Rect {
        let rect = Rect::from_min_size(self.cursor, size);
        match self.direction {
            Direction::TopDown => {
                self.cursor.y += size.y + self.spacing;
            }
            Direction::LeftToRight => {
                self.cursor.x += size.x + self.spacing;
            }
        }
        self.used = self.used.union(rect);
        rect
    }

    /// Add empty space along the layout direction.
    pub fn add_space(&mut self, amount: f32) {
        match self.direction {
            Direction::TopDown => self.cursor.y += amount,
            Direction::LeftToRight => self.cursor.x += amount,
        }
    }

    /// Draw a thin dividing line across the layout: horizontal (full
    /// available width) in top-down layouts, vertical (one widget-row
    /// tall — a horizontal row's available height can be the parent's
    /// whole remaining space) in left-to-right.
    pub fn separator(&mut self) {
        let style = self.ctx.style;
        let color = style.palette.stroke;
        let rect = match self.direction {
            Direction::TopDown => {
                self.allocate(Vec2::new(self.available().width(), 1.0))
            }
            Direction::LeftToRight => {
                let h = style.fonts.body + style.spacing.button_padding.y * 2.0;
                self.allocate(Vec2::new(1.0, h))
            }
        };
        let shape = RectShape::fill(rect, 0.0, color);
        self.ctx.paint.push(PaintCmd::Rrect(shape));
    }

    pub fn used_rect(&self) -> Rect {
        self.used
    }

    /// Move the cursor to an absolute position. Low-level escape hatch for
    /// container widgets (grids, tables) that place children explicitly
    /// rather than flowing them from the parent cursor.
    pub fn set_cursor(&mut self, pos: Pos2) {
        self.cursor = pos;
    }

    /// Run `f` in a fresh child layout occupying `rect`, namespaced under
    /// `root_id`, and report what it actually used. Does **not** touch this
    /// Ui's cursor or `used` box — the caller decides how the child's
    /// footprint affects the parent layout (e.g. a grid sums cell widths).
    /// The child inherits this Ui's floating-area binding for hit testing.
    pub fn run_at<R>(
        &mut self,
        rect: Rect,
        direction: Direction,
        root_id: Id,
        opts: UiOptions,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> (R, Rect) {
        let mut child =
            Ui::with_root(&mut *self.ctx, &mut *self.text, rect, direction, opts, root_id);
        child.area = self.area;
        child.clip = self.clip;
        let result = f(&mut child);
        (result, child.used_rect())
    }

    /// Run `f` in a nested left-to-right layout starting at the current
    /// cursor. The cursor advances in the parent direction by the bounding
    /// box of whatever the children produced.
    pub fn horizontal<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.run_nested(Direction::LeftToRight, f)
    }

    /// Run `f` in a nested top-down layout starting at the current cursor.
    pub fn vertical<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.run_nested(Direction::TopDown, f)
    }

    fn run_nested<R>(
        &mut self,
        new_dir: Direction,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let saved_cursor = self.cursor;
        let saved_direction = self.direction;
        let saved_used = self.used;

        self.direction = new_dir;
        self.used = Rect::from_min_size(saved_cursor, Vec2::ZERO);

        let result = f(self);

        let child_used = self.used;

        // Restore parent layout state.
        self.cursor = saved_cursor;
        self.direction = saved_direction;
        self.used = saved_used;

        // Advance the parent cursor by the child's bounding box.
        let child_size = child_used.size();
        if child_size.x > 0.0 && child_size.y > 0.0 {
            let _ = self.allocate(child_size);
        }

        result
    }

    /// Run `f` as a left-to-right group positioned horizontally within the
    /// available width according to `align`. Useful for right-aligned
    /// toolbars or centered titles.
    ///
    /// Alignment shifts the *cursor* (not the emitted paint), so hit-test
    /// rects stay consistent with what's drawn. The group's width is only
    /// known after it runs, so — like [`Grid`](crate::widgets::Grid) — the
    /// measured width is persisted and applied next frame: the first frame a
    /// group appears it renders left-aligned, then snaps to its alignment.
    pub fn horizontal_aligned<R>(&mut self, align: Align, f: impl FnOnce(&mut Ui) -> R) -> R {
        let id = self.alloc_id("h_aligned");
        let prev_w = self.ctx.memory.data_get::<f32>(id).copied().unwrap_or(0.0);

        let avail_w = self.available_size().x;
        let dx = match align {
            Align::Min => 0.0,
            Align::Center => ((avail_w - prev_w) * 0.5).max(0.0),
            Align::Max => (avail_w - prev_w).max(0.0),
        };

        let origin = self.cursor;
        let saved_direction = self.direction;
        let saved_used = self.used;

        let start = Pos2::new(origin.x + dx, origin.y);
        self.cursor = start;
        self.direction = Direction::LeftToRight;
        self.used = Rect::from_min_size(start, Vec2::ZERO);

        let result = f(self);
        let group = self.used;

        self.direction = saved_direction;
        self.used = saved_used;
        self.cursor = origin;

        let size = group.size();
        self.ctx.memory.data_insert::<f32>(id, size.x);

        if size.x > 0.0 && size.y > 0.0 {
            self.used = self.used.union(group);
            match saved_direction {
                Direction::TopDown => self.cursor.y = origin.y + size.y + self.spacing,
                Direction::LeftToRight => {
                    self.cursor.x = origin.x + dx + size.x + self.spacing
                }
            }
        }

        result
    }
}
