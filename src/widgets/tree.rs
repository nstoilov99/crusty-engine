use crate::context::Ui;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;

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
pub struct TreeNode<'a, T: PartialEq + Copy> {
    selection: &'a mut T,
    value: T,
    label: String,
    default_open: bool,
}

impl<'a, T: PartialEq + Copy> TreeNode<'a, T> {
    pub fn new(selection: &'a mut T, value: T, label: impl Into<String>) -> Self {
        Self {
            selection,
            value,
            label: label.into(),
            default_open: false,
        }
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
    ) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(("tree_node", &self.label));

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

        let mut is_open = ui
            .ctx()
            .memory
            .get(id)
            .map(|m| m.bool_state)
            .unwrap_or(self.default_open);

        let mut chevron_clicked = false;
        let mut label_clicked = false;
        if pointer_released && is_active {
            if chev_hovered {
                chevron_clicked = true;
            } else if label_hovered {
                label_clicked = true;
            }
        }
        if chevron_clicked {
            is_open = !is_open;
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
            let mut c = style.palette.accent;
            c.a = 0.75;
            c
        } else if row_hovered {
            style.palette.surface_hover
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
            Color::rgba(0.10, 0.10, 0.12, 0.95)
        } else {
            style.palette.text_dim
        };
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

        // Label
        let label_x = chevron_rect.max.x + gap;
        let label_y = row_rect.min.y + (row_h - label_size.y) * 0.5;
        let label_color = if is_selected {
            Color::rgba(0.10, 0.10, 0.12, 0.95)
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

        Response {
            id,
            rect: row_rect,
            hovered: row_hovered,
            pressed: is_active,
            clicked: label_clicked,
            hover_anim: if row_hovered { 1.0 } else { 0.0 },
            press_anim: 0.0,
        }
    }
}
