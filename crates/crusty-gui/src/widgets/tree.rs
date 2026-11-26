use crate::context::Ui;
use crate::id::Id;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

/// Where a dragged tree payload was dropped relative to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeDropPos {
    /// Insert as a sibling above the row (reorder).
    Before,
    /// Make it a child of the row (reparent).
    Into,
    /// Insert as a sibling below the row (reorder).
    After,
}

/// Result of [`TreeNode::show_dnd`].
pub struct TreeNodeOutput<T> {
    pub response: Response,
    /// `Some((dragged_value, position))` when a dragged payload of type
    /// `T` was released on this row this frame. The caller applies the
    /// actual model change (reparent / reorder).
    pub drop: Option<(T, TreeDropPos)>,
}

/// A hierarchical tree row: chevron button on the left, selectable label
/// taking the rest of the row.
///
/// * Click on the **chevron area** → toggle open/closed.
/// * Click on the **label area** → set `*selection = value`.
/// * The row highlights when hovered, and is rendered with the accent
///   fill when `*selection == value`.
/// * Leaf items (those with no children) should use
///   [`SelectableValue`](crate::widgets::SelectableValue) inside the
///   parent's body — they inherit the indent automatically.
/// * With [`draggable`](Self::draggable), the row is a drag source
///   carrying `value`; every row is a drop target for payloads of type
///   `T` (top quarter = before, bottom quarter = after, middle = into).
///   Use [`show_dnd`](Self::show_dnd) to receive drops.
pub struct TreeNode<'a, T: PartialEq + Copy> {
    selection: &'a mut T,
    value: T,
    label: String,
    default_open: bool,
    draggable: bool,
    leaf: bool,
    id_override: Option<Id>,
}

impl<'a, T: PartialEq + Copy + 'static> TreeNode<'a, T> {
    pub fn new(selection: &'a mut T, value: T, label: impl Into<String>) -> Self {
        Self {
            selection,
            value,
            label: label.into(),
            default_open: false,
            draggable: false,
            leaf: false,
            id_override: None,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Make the row a drag source carrying `value` (for reparent /
    /// reorder). Drops are reported by [`show_dnd`](Self::show_dnd).
    pub fn draggable(mut self, on: bool) -> Self {
        self.draggable = on;
        self
    }

    /// Render as a leaf: no chevron, no body, the whole row selects.
    /// Still a full drag source / drop target, so a childless node can be
    /// dragged elsewhere or receive its first child.
    pub fn leaf(mut self, on: bool) -> Self {
        self.leaf = on;
        self
    }

    /// Derive the widget id from `source` instead of the label + call
    /// order. Use a stable unique key (an entity id) whenever rows can be
    /// reordered or share labels — the default positional id would make
    /// open/drag state stick to the row *position* rather than the node.
    pub fn id_source(mut self, source: impl std::hash::Hash) -> Self {
        self.id_override = Some(Id::new(("tree_node", source)));
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui, &mut T) -> R) -> Response {
        self.show_dnd(ui, body).response
    }

    /// Like [`show`](Self::show), but also reports drag-and-drop payloads
    /// released on this row.
    pub fn show_dnd<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
    ) -> TreeNodeOutput<T> {
        let style = ui.style();
        let id = match self.id_override {
            Some(id) => id,
            None => ui.alloc_id(("tree_node", &self.label)),
        };

        // Initialise default_open on first sight.
        if ui.ctx().memory.get(id).is_none() {
            ui.ctx_mut().memory.get_or_default(id).bool_state = self.default_open;
        }

        // ── Layout
        let chevron_w = style.fonts.body + 6.0;
        let gap = style.spacing.box_label_gap;
        let label_size = ui.text_mut().measure(&self.label, style.fonts.body, None);
        let row_h = label_size.y.max(style.fonts.body) + 6.0;
        let row_rect = ui.allocate(Vec2::new(ui.available().width(), row_h));
        let chevron_rect = Rect::from_min_max(
            row_rect.min,
            Pos2::new(row_rect.min.x + chevron_w, row_rect.max.y),
        );
        let label_rect = Rect::from_min_max(
            Pos2::new(row_rect.min.x + chevron_w, row_rect.min.y),
            row_rect.max,
        );

        // ── Input
        let chev_hovered = ui.contains_pointer(chevron_rect);
        let label_hovered = ui.contains_pointer(label_rect);
        let row_hovered = chev_hovered || label_hovered;
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let active_id = ui.ctx().memory.active_widget;
        if pointer_pressed && row_hovered {
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let is_active = active_id == Some(id) || (pointer_pressed && row_hovered);

        // Drag source: arms on press, activates once the pointer travels
        // past the drag threshold. While a drag is in flight the release
        // must not count as a click.
        if self.draggable {
            let value = self.value;
            ui.dnd_drag_source(id, row_rect, self.label.clone(), || value);
        }
        let drag_in_flight = ui.ctx().dnd.is_dragging();

        let mut is_open = ui
            .ctx()
            .memory
            .get(id)
            .map(|m| m.bool_state)
            .unwrap_or(self.default_open);

        let mut chevron_clicked = false;
        let mut label_clicked = false;
        if pointer_released && is_active && !drag_in_flight {
            if chev_hovered && !self.leaf {
                chevron_clicked = true;
            } else if label_hovered || (chev_hovered && self.leaf) {
                label_clicked = true;
            }
        }
        if chevron_clicked {
            is_open = !is_open;
        }
        if self.leaf {
            is_open = false;
        }
        if label_clicked {
            *self.selection = self.value;
        }
        let is_selected = *self.selection == self.value;

        // Persist
        {
            let mem = ui.ctx_mut().memory.get_or_default(id);
            mem.seen_this_frame = true;
            mem.last_rect = row_rect;
            mem.bool_state = is_open;
        }

        // ── Render
        let bg_color = if is_selected {
            style.palette.selection_fill
        } else if row_hovered {
            style.palette.hover
        } else {
            Color::TRANSPARENT
        };
        if bg_color.a > 0.0 {
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                row_rect,
                style.rounding.small,
                bg_color,
            )));
        }

        // Chevron — two strokes, not a glyph: stays crisp, monochrome, and
        // tinted by `chev_color` (a glyph chevron risks an emoji substitution
        // for arrow codepoints).
        let chev_color = if is_selected {
            style.palette.selection_text
        } else {
            style.palette.text_secondary
        };
        if !self.leaf {
            let center = Pos2::new(
                chevron_rect.min.x + chevron_w * 0.5,
                chevron_rect.min.y + row_h * 0.5,
            );
            crate::widgets::paint_chevron(
                &mut ui.ctx_mut().paint,
                center,
                style.fonts.body * 0.30,
                (style.fonts.body * 0.12).max(1.5),
                is_open,
                chev_color,
            );
        }

        // Label
        let label_x = chevron_rect.max.x + gap;
        let label_y = row_rect.min.y + (row_h - label_size.y) * 0.5;
        let label_color = if is_selected {
            style.palette.selection_text
        } else {
            style.palette.text
        };
        {
            let Ui { ctx, text, .. } = ui;
            text.add_text(
                &mut ctx.paint,
                &self.label,
                style.fonts.body,
                Pos2::new(label_x, label_y),
                label_color,
                None,
            );
        }

        // ── Drop target: top quarter = before, bottom quarter = after,
        // middle half = into. Skipped when the payload is this row itself.
        let mut drop = None;
        if ui.ctx().dnd.holds::<T>()
            && ui.contains_pointer(row_rect)
            && ui.ctx().dnd.peek::<T>() != Some(&self.value)
            && let Some(p) = ui.ctx().input.pointer_pos
        {
            let frac = (p.y - row_rect.min.y) / row_h;
            let pos = if frac < 0.25 {
                TreeDropPos::Before
            } else if frac > 0.75 {
                TreeDropPos::After
            } else {
                TreeDropPos::Into
            };

            let accent = style.palette.accent_active;
            let indicator = match pos {
                TreeDropPos::Before => PaintCmd::Rrect(RectShape::fill(
                    Rect::from_min_size(
                        Pos2::new(row_rect.min.x, row_rect.min.y - 1.0),
                        Vec2::new(row_rect.width(), 2.0),
                    ),
                    Rounding::same(1.0),
                    accent,
                )),
                TreeDropPos::After => PaintCmd::Rrect(RectShape::fill(
                    Rect::from_min_size(
                        Pos2::new(row_rect.min.x, row_rect.max.y - 1.0),
                        Vec2::new(row_rect.width(), 2.0),
                    ),
                    Rounding::same(1.0),
                    accent,
                )),
                TreeDropPos::Into => PaintCmd::Rrect(
                    RectShape::fill(row_rect, style.rounding.small, style.palette.accent_soft)
                        .with_stroke(1.5, accent),
                ),
            };
            ui.ctx_mut().paint.push(indicator);

            if pointer_released {
                drop = ui.ctx_mut().dnd.take::<T>().map(|v| (v, pos));
            }
        }

        // Body (indented), passing selection through so children can use
        // selectable_value without re-borrowing it.
        if is_open {
            let indent = (style.fonts.body + gap) * 1.6;
            ui.add_space(2.0);
            let selection = self.selection;
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.vertical(|ui| {
                    body(ui, selection);
                });
            });
        }

        TreeNodeOutput {
            response: Response {
                id,
                rect: row_rect,
                hovered: row_hovered,
                pressed: is_active,
                clicked: label_clicked,
                hover_anim: if row_hovered { 1.0 } else { 0.0 },
                press_anim: 0.0,
            },
            drop,
        }
    }
}
