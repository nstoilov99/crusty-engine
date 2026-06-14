//! Cursor-based immediate-mode UI handle.
//!
//! A [`Ui`] borrows the [`Context`](super::Context) and a
//! [`TextRenderer`](crate::text::TextRenderer) for the duration of a
//! single layout pass, and tracks a "cursor" that advances as widgets
//! allocate space inside the available rect.

use crate::context::Context;
use crate::id::Id;
use crate::math::{Pos2, Rect, Vec2};
use crate::text::TextRenderer;

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

    /// Occlusion-aware pointer test: true when the pointer is inside
    /// `rect` and no floating area above this Ui's layer (or a modal)
    /// owns the pointer. Widgets use this instead of raw
    /// `input.pointer_in`.
    pub fn contains_pointer(&self, rect: Rect) -> bool {
        self.ctx.pointer_over(self.area, rect)
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
