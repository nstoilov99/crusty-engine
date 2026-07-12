//! Menu bar, dropdown menus, nested submenus, and clickable menu items.
//!
//! Menus reuse the same floating-popup machinery as
//! [`context_menu_for`](crate::widgets::context_menu_for): the body paints
//! into a scratch buffer that's drained into `overlay_paint` and filed as a
//! `Tooltip`-order area so hit testing treats it as topmost. Top-level menus
//! and submenus toggle on click; selecting a [`menu_item`](Ui::menu_item)
//! (or any `SelectableValue`) sets `popup_close_requested`, which cascades up
//! and closes the whole chain.

use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::input::Key;
use crate::layer::Order;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

const MENU_WIDTH: f32 = 200.0;

impl Ui<'_> {
    /// Lay out a horizontal menu bar across the available width. Put
    /// [`menu_button`](Ui::menu_button) calls inside `body`.
    pub fn menu_bar(&mut self, body: impl FnOnce(&mut Ui)) {
        let style = self.style();
        let height = style.fonts.body + 12.0;
        let strip = Rect::from_min_size(
            self.cursor(),
            Vec2::new(self.available().width(), height),
        );
        self.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
            strip,
            style.rounding.small,
            style.palette.surface,
        )));
        self.horizontal(body);
    }

    /// A top-level menu button. Click toggles a dropdown below it whose
    /// contents come from `body`.
    pub fn menu_button(&mut self, label: impl AsRef<str>, body: impl FnOnce(&mut Ui)) {
        let label = label.as_ref();
        let style = self.style();
        let id = self.alloc_id(("menu_button", label));

        let text_size = self.text_mut().measure(label, style.fonts.body, None);
        let pad = Vec2::new(8.0, 5.0);
        let rect = self.allocate(Vec2::new(
            text_size.x + pad.x * 2.0,
            text_size.y + pad.y * 2.0,
        ));

        let hovered = self.contains_pointer(rect);
        let mut is_open = self.ctx().memory.get(id).map_or(false, |m| m.bool_state);
        if self.ctx().input.pointer_pressed && hovered {
            is_open = !is_open;
        }

        let fill = if is_open {
            style.palette.surface_active
        } else if hovered {
            style.palette.surface_hover
        } else {
            Color::TRANSPARENT
        };
        if fill.a > 0.0 {
            self.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                rect,
                style.rounding.small,
                fill,
            )));
        }
        let text_pos = Pos2::new(
            rect.min.x + pad.x,
            rect.min.y + (rect.height() - text_size.y) * 0.5,
        );
        {
            let Ui { ctx, text, .. } = self;
            text.add_text(
                &mut ctx.paint,
                label,
                style.fonts.body,
                text_pos,
                style.palette.text,
                None,
            );
        }

        if is_open {
            let anchor = Pos2::new(rect.min.x, rect.max.y);
            if menu_panel(self, id.with("panel"), anchor, rect, MENU_WIDTH, body) {
                is_open = false;
            }
        }

        let mem = self.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.bool_state = is_open;
    }

    /// A submenu row inside a menu body. Click toggles a flyout panel to the
    /// right whose contents come from `body`.
    pub fn submenu(&mut self, label: impl AsRef<str>, body: impl FnOnce(&mut Ui)) {
        self.submenu_width(label, MENU_WIDTH, body);
    }

    /// [`submenu`](Ui::submenu) with an explicit flyout panel width, for
    /// bodies wider than the default (embedded editors, forms).
    pub fn submenu_width(
        &mut self,
        label: impl AsRef<str>,
        width: f32,
        body: impl FnOnce(&mut Ui),
    ) {
        let label = label.as_ref();
        let style = self.style();
        let id = self.alloc_id(("submenu", label));

        let text_size = self.text_mut().measure(label, style.fonts.body, None);
        let rect = self.allocate(Vec2::new(self.available().width(), text_size.y + 6.0));

        let hovered = self.contains_pointer(rect);
        let mut is_open = self.ctx().memory.get(id).map_or(false, |m| m.bool_state);
        if self.ctx().input.pointer_pressed && hovered {
            is_open = !is_open;
        }

        let fill = if hovered || is_open {
            style.palette.surface_active
        } else {
            Color::TRANSPARENT
        };
        if fill.a > 0.0 {
            self.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                rect,
                style.rounding.small,
                fill,
            )));
        }
        let text_pos = Pos2::new(rect.min.x + 8.0, rect.min.y + 3.0);
        {
            let Ui { ctx, text, .. } = self;
            text.add_text(
                &mut ctx.paint,
                label,
                style.fonts.body,
                text_pos,
                style.palette.text,
                None,
            );
        }
        // Right-pointing chevron to mark the submenu.
        let cy = rect.min.y + rect.height() * 0.5;
        let cx = rect.max.x - 10.0;
        super::paint_chevron(
            &mut self.ctx_mut().paint,
            Pos2::new(cx, cy),
            4.0,
            1.5,
            false,
            style.palette.text_dim,
        );

        if is_open {
            let anchor = Pos2::new(rect.max.x + 2.0, rect.min.y - 4.0);
            if menu_panel(self, id.with("panel"), anchor, rect, width, body) {
                is_open = false;
            }
        }

        let mem = self.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.bool_state = is_open;
    }

    /// A clickable menu entry. Returns `true` on the frame it's clicked and
    /// asks the enclosing menu chain to close.
    pub fn menu_item(&mut self, label: impl AsRef<str>) -> bool {
        self.menu_item_enabled(label, true)
    }

    /// [`menu_item`](Ui::menu_item) that can be disabled: dimmed text, no
    /// hover highlight, never reports a click.
    pub fn menu_item_enabled(&mut self, label: impl AsRef<str>, enabled: bool) -> bool {
        let label = label.as_ref();
        let style = self.style();
        let id = self.alloc_id(("menu_item", label));

        let text_size = self.text_mut().measure(label, style.fonts.body, None);
        let rect = self.allocate(Vec2::new(self.available().width(), text_size.y + 6.0));

        if !enabled {
            let text_pos = Pos2::new(rect.min.x + 8.0, rect.min.y + 3.0);
            let Ui { ctx, text, .. } = self;
            text.add_text(
                &mut ctx.paint,
                label,
                style.fonts.body,
                text_pos,
                style.palette.text_dim,
                None,
            );
            return false;
        }

        let hovered = self.contains_pointer(rect);
        let pointer_pressed = self.ctx().input.pointer_pressed;
        let pointer_released = self.ctx().input.pointer_released;
        let active_id = self.ctx().memory.active_widget;
        if pointer_pressed && hovered {
            self.ctx_mut().memory.active_widget = Some(id);
        }
        let is_active = active_id == Some(id) || (pointer_pressed && hovered);
        let mut clicked = false;
        if pointer_released && is_active && hovered {
            clicked = true;
            self.ctx_mut().memory.popup_close_requested = true;
        }

        // The dropdown panel is itself painted with `surface_hover`, so a
        // `surface_hover` row would be invisible. Use `surface_active` for
        // hover and an accent wash while the button is pressed so the user
        // gets clear hover *and* click feedback.
        let highlight = if is_active {
            Some(style.palette.accent.with_alpha(0.45))
        } else if hovered {
            Some(style.palette.surface_active)
        } else {
            None
        };
        if let Some(fill) = highlight {
            self.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                rect,
                style.rounding.small,
                fill,
            )));
        }
        let text_pos = Pos2::new(rect.min.x + 8.0, rect.min.y + 3.0);
        let Ui { ctx, text, .. } = self;
        text.add_text(
            &mut ctx.paint,
            label,
            style.fonts.body,
            text_pos,
            style.palette.text,
            None,
        );
        clicked
    }
}

/// Render a floating menu panel at `anchor` containing `body`, filed as a
/// topmost area. `trigger` is the row/button that opened it, so a click on
/// the trigger itself doesn't count as an outside-click. Returns `true` when
/// the panel should close (selection made, Esc, or click outside both the
/// panel and its trigger).
fn menu_panel(
    ui: &mut Ui,
    id: Id,
    anchor: Pos2,
    trigger: Rect,
    width: f32,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let style = ui.style();
    let area_id = id.with("area");
    let scratch = Rect::from_min_size(anchor, Vec2::new(width, 2000.0));

    let body_start = ui.ctx().paint.len();
    let overlay_start = ui.ctx().overlay_paint.len();
    let used_h = {
        let Ui { ctx, text, .. } = ui;
        let opts = UiOptions {
            padding: Vec2::new(4.0, 4.0),
            spacing: 2.0,
        };
        let mut inner =
            Ui::with_root(ctx, text, scratch, Direction::TopDown, opts, id.with("root"));
        inner.set_area(Some(area_id));
        body(&mut inner);
        inner.used_rect().height()
    };
    let drained: Vec<PaintCmd> = ui.ctx_mut().paint.drain(body_start..).collect();

    let panel = Rect::from_min_size(
        anchor,
        Vec2::new(width, (used_h + 8.0).max(20.0)),
    );

    // Nested popups (submenus, combo dropdowns) pushed into `overlay_paint`
    // while the body ran; they must render above this panel's bg/shadow, so
    // pull them out, paint the panel, then re-append them.
    let children: Vec<PaintCmd> = ui.ctx_mut().overlay_paint.drain(overlay_start..).collect();
    ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
        RectShape::fill(panel, style.rounding.widget, style.palette.surface_hover)
            .with_stroke(1.0, style.palette.stroke)
            .with_shadow(20.0, Color::rgba(0.0, 0.0, 0.0, 0.45)),
    ));
    ui.ctx_mut().overlay_paint.extend(drained);
    ui.ctx_mut().overlay_paint.extend(children);

    let area = ui.ctx_mut().memory.area_entry(area_id, panel.min, Order::Tooltip);
    area.pos = panel.min;
    area.size = panel.size();

    let mut closed = ui.ctx().memory.popup_close_requested
        || ui.ctx().input.key_pressed(Key::Escape);
    if ui.ctx().input.pointer_pressed {
        if let Some(p) = ui.ctx().input.pointer_pos {
            // A click in ANY topmost area (this panel, a nested flyout, a
            // combo popup) is not an outside-click — otherwise clicking a
            // child popup would collapse the whole menu chain.
            if !trigger.contains(p) && !ui.ctx().memory.topmost_area_contains(p) {
                closed = true;
            }
        }
    }
    closed
}
