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
        }
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
}
