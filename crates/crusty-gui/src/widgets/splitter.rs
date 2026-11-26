use crate::context::{CursorIcon, Direction, Ui, UiOptions};
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

const THICKNESS: f32 = 5.0;
const HIT_PADDING: f32 = 2.0;

/// Persistent split ratio, kept in the typed `Memory` store so drags
/// survive frames.
#[derive(Clone, Copy, Debug)]
struct SplitterState {
    ratio: f32,
}

/// Two resizable panes separated by a draggable bar, filling the Ui's
/// remaining space.
///
/// ```ignore
/// Splitter::horizontal("browser").default_ratio(0.2).min_sizes(120.0, 200.0)
///     .show(ui, |left| { /* folder tree */ }, |right| { /* content */ });
/// ```
pub struct Splitter {
    id_source: String,
    /// Panes side-by-side (vertical bar) when true, stacked otherwise.
    side_by_side: bool,
    default_ratio: f32,
    min_first: f32,
    min_second: f32,
}

impl Splitter {
    /// Side-by-side panes with a vertical drag bar.
    pub fn horizontal(id_source: impl Into<String>) -> Self {
        Self::new(id_source, true)
    }

    /// Stacked panes with a horizontal drag bar.
    pub fn vertical(id_source: impl Into<String>) -> Self {
        Self::new(id_source, false)
    }

    fn new(id_source: impl Into<String>, side_by_side: bool) -> Self {
        Self {
            id_source: id_source.into(),
            side_by_side,
            default_ratio: 0.5,
            min_first: 32.0,
            min_second: 32.0,
        }
    }

    /// First pane's share of the space before the user drags (0..1).
    pub fn default_ratio(mut self, r: f32) -> Self {
        self.default_ratio = r;
        self
    }

    /// Smallest pixel extents the panes can be dragged to.
    pub fn min_sizes(mut self, first: f32, second: f32) -> Self {
        self.min_first = first;
        self.min_second = second;
        self
    }

    pub fn show<A, B>(
        self,
        ui: &mut Ui,
        first: impl FnOnce(&mut Ui) -> A,
        second: impl FnOnce(&mut Ui) -> B,
    ) -> (A, B) {
        let style = ui.style();
        let rect = ui.allocate(ui.available_size());
        let id = ui.alloc_id(("splitter", &self.id_source));
        let bar_id = id.with("bar");

        let mut ratio = ui
            .ctx()
            .memory
            .data_get::<SplitterState>(id)
            .map(|s| s.ratio)
            .unwrap_or(self.default_ratio);

        // Drag updates the ratio before rects are computed so the bar
        // tracks the pointer within the same frame.
        let extent = if self.side_by_side {
            rect.width()
        } else {
            rect.height()
        };
        let bar_rect = |r: f32| bar_rect(rect, self.side_by_side, r);
        let resp = ui.interact(bar_id, bar_rect(ratio).expand(HIT_PADDING));
        if resp.pressed
            && let Some(p) = ui.ctx().input.pointer_pos
        {
            ratio = if self.side_by_side {
                (p.x - rect.min.x) / rect.width()
            } else {
                (p.y - rect.min.y) / rect.height()
            };
        }
        if extent > self.min_first + self.min_second + THICKNESS {
            ratio = ratio.clamp(
                self.min_first / extent,
                1.0 - (self.min_second + THICKNESS) / extent,
            );
        } else {
            ratio = ratio.clamp(0.05, 0.95);
        }
        if resp.hovered || resp.pressed {
            ui.ctx_mut().set_cursor_icon(if self.side_by_side {
                CursorIcon::ResizeEw
            } else {
                CursorIcon::ResizeNs
            });
        }

        let bar = bar_rect(ratio);
        let (a, b) = pane_rects(rect, self.side_by_side, bar);

        let highlight = if resp.pressed {
            1.0
        } else if resp.hovered {
            0.55
        } else {
            0.0
        };
        let color = lerp_color(style.palette.stroke, style.palette.accent_active, highlight);
        // Idle: subtle 1px divider; hover/drag: the full grab bar lights up.
        let visual = if highlight > 0.0 {
            bar
        } else {
            thin_bar(bar, self.side_by_side)
        };
        ui.ctx_mut()
            .paint
            .push(PaintCmd::Rrect(RectShape::fill(visual, 0.0, color)));

        let opts = UiOptions {
            padding: Vec2::ZERO,
            spacing: style.spacing.item,
        };
        let parent_clip = ui.clip_rect();
        let (ra, _) = ui.run_at(a, Direction::TopDown, id.with("first"), opts, |child| {
            child.set_clip(a.intersect(parent_clip));
            child.ctx_mut().paint.push(PaintCmd::PushClip(a));
            let r = first(child);
            child.ctx_mut().paint.push(PaintCmd::PopClip);
            r
        });
        let (rb, _) = ui.run_at(b, Direction::TopDown, id.with("second"), opts, |child| {
            child.set_clip(b.intersect(parent_clip));
            child.ctx_mut().paint.push(PaintCmd::PushClip(b));
            let r = second(child);
            child.ctx_mut().paint.push(PaintCmd::PopClip);
            r
        });

        ui.ctx_mut().memory.data_insert(id, SplitterState { ratio });
        (ra, rb)
    }
}

fn bar_rect(rect: Rect, side_by_side: bool, ratio: f32) -> Rect {
    let half = THICKNESS * 0.5;
    if side_by_side {
        let x = rect.min.x + rect.width() * ratio;
        Rect::from_min_max(
            Pos2::new(x - half, rect.min.y),
            Pos2::new(x + half, rect.max.y),
        )
    } else {
        let y = rect.min.y + rect.height() * ratio;
        Rect::from_min_max(
            Pos2::new(rect.min.x, y - half),
            Pos2::new(rect.max.x, y + half),
        )
    }
}

fn thin_bar(bar: Rect, side_by_side: bool) -> Rect {
    if side_by_side {
        let x = bar.center().x;
        Rect::from_min_max(Pos2::new(x - 0.5, bar.min.y), Pos2::new(x + 0.5, bar.max.y))
    } else {
        let y = bar.center().y;
        Rect::from_min_max(Pos2::new(bar.min.x, y - 0.5), Pos2::new(bar.max.x, y + 0.5))
    }
}

fn pane_rects(rect: Rect, side_by_side: bool, bar: Rect) -> (Rect, Rect) {
    if side_by_side {
        (
            Rect::from_min_max(rect.min, Pos2::new(bar.min.x, rect.max.y)),
            Rect::from_min_max(Pos2::new(bar.max.x, rect.min.y), rect.max),
        )
    } else {
        (
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, bar.min.y)),
            Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y), rect.max),
        )
    }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * t.clamp(0.0, 1.0);
    Color::rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}
