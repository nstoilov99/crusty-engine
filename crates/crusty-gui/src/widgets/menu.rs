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
use crate::layer::Order;
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

const MENU_WIDTH: f32 = 200.0;
/// Menu rows are a fixed 24px tall (Crusty Design System).
const MENU_ROW_H: f32 = 24.0;

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
            style.palette.header,
        )));
        self.horizontal(body);
    }

    /// A top-level menu button. Click toggles a dropdown below it whose
    /// contents come from `body`.
    pub fn menu_button(&mut self, label: impl AsRef<str>, body: impl FnOnce(&mut Ui)) {
        self.menu_button_width(label, MENU_WIDTH, body);
    }

    /// [`menu_button`](Ui::menu_button) with an explicit dropdown width, for
    /// menus whose rows carry shortcut columns (Edit menu: 250px).
    pub fn menu_button_width(
        &mut self,
        label: impl AsRef<str>,
        width: f32,
        body: impl FnOnce(&mut Ui),
    ) {
        let label = label.as_ref();
        let style = self.style();
        let id = self.alloc_id(("menu_button", label));

        let text_size = self.text_mut().measure(label, style.fonts.body, None);
        let pad = Vec2::new(8.0, 5.0);
        let rect = self.allocate(Vec2::new(
            text_size.x + pad.x * 2.0,
            text_size.y + pad.y * 2.0,
        ));

        // Shared slot naming the currently open top-level menu, so sibling
        // buttons can hover-switch (standard menu-mode behavior).
        let bar_key = Id::ROOT.with("menu_bar_open");
        let open_sibling = self.ctx().memory.data_get::<Id>(bar_key).copied();

        let hovered = self.contains_pointer(rect);
        let mut is_open = self.ctx().memory.get(id).is_some_and(|m| m.bool_state);
        // Another menu took over (hover-switch) since last frame: close.
        if is_open && open_sibling.is_some_and(|o| o != id) {
            is_open = false;
        }
        if self.ctx().input.pointer_pressed && hovered {
            is_open = !is_open;
        } else if hovered && !is_open && open_sibling.is_some_and(|o| o != id) {
            // With a sibling menu open, hovering this button switches to it.
            is_open = true;
        }

        let fill = if is_open {
            style.palette.active
        } else if hovered {
            style.palette.hover
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
            if menu_panel(self, id.with("panel"), anchor, rect, width, body) {
                is_open = false;
            }
        }

        if is_open {
            self.ctx_mut().memory.data_insert(bar_key, id);
        } else if open_sibling == Some(id) {
            self.ctx_mut().memory.data_remove::<Id>(bar_key);
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
        let mut is_open = self.ctx().memory.get(id).is_some_and(|m| m.bool_state);
        if self.ctx().input.pointer_pressed && hovered {
            is_open = !is_open;
        }

        let fill = if hovered || is_open {
            style.palette.active
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
            style.palette.text_secondary,
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
        self.menu_row(label.as_ref(), None, true)
    }

    /// [`menu_item`](Ui::menu_item) that can be disabled: dimmed text, no
    /// hover highlight, never reports a click.
    pub fn menu_item_enabled(&mut self, label: impl AsRef<str>, enabled: bool) -> bool {
        self.menu_row(label.as_ref(), None, enabled)
    }

    /// Menu entry with a right-aligned mono shortcut column ("Ctrl+Z").
    pub fn menu_item_shortcut(
        &mut self,
        label: impl AsRef<str>,
        shortcut: impl AsRef<str>,
        enabled: bool,
    ) -> bool {
        self.menu_row(label.as_ref(), Some(shortcut.as_ref()), enabled)
    }

    /// Uppercase mono group header inside a menu (History / Edit / ...).
    pub fn menu_group_header(&mut self, label: impl AsRef<str>) {
        let style = self.style();
        let font = style.fonts.small;
        let text_str = label.as_ref().to_uppercase();
        let rect = self.allocate(Vec2::new(
            self.available().width(),
            font * 1.25 + 8.0,
        ));
        let Ui { ctx, text, .. } = self;
        text.add_text_family(
            &mut ctx.paint,
            &text_str,
            font,
            Pos2::new(rect.min.x + 10.0, rect.min.y + 6.0),
            style.palette.text_secondary,
            None,
            crate::text::FontFamily::Mono,
        );
    }

    fn menu_row(&mut self, label: &str, shortcut: Option<&str>, enabled: bool) -> bool {
        let style = self.style();
        let id = self.alloc_id(("menu_item", label));

        let text_size = self.text_mut().measure(label, style.fonts.body, None);
        let rect = self.allocate(Vec2::new(self.available().width(), MENU_ROW_H));
        let text_pos = Pos2::new(
            rect.min.x + 10.0,
            rect.min.y + (MENU_ROW_H - text_size.y) * 0.5,
        );

        let mut clicked = false;
        if enabled {
            let hovered = self.contains_pointer(rect);
            let pointer_pressed = self.ctx().input.pointer_pressed;
            let pointer_released = self.ctx().input.pointer_released;
            let active_id = self.ctx().memory.active_widget;
            if pointer_pressed && hovered {
                self.ctx_mut().memory.active_widget = Some(id);
            }
            let is_active = active_id == Some(id) || (pointer_pressed && hovered);
            if pointer_released && is_active && hovered {
                clicked = true;
                self.ctx_mut().memory.popup_close_requested = true;
            }

            // The dropdown panel is itself painted with `surface_hover`, so a
            // `surface_hover` row would be invisible. Use `surface_active` for
            // hover and an accent wash while the button is pressed so the user
            // gets clear hover *and* click feedback.
            let highlight = if is_active {
                Some(style.palette.accent_active.with_alpha(0.45))
            } else if hovered {
                Some(style.palette.active)
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
        }

        let label_color = if enabled {
            style.palette.text
        } else {
            style.palette.text_disabled
        };
        let shortcut_draw = shortcut.map(|s| {
            let font = style.fonts.mono;
            let w = self
                .text_mut()
                .measure_family(s, font, None, crate::text::FontFamily::Mono)
                .x;
            let color = if enabled {
                style.palette.text_secondary
            } else {
                style.palette.stroke_strong
            };
            (
                Pos2::new(
                    rect.max.x - 10.0 - w,
                    rect.min.y + (MENU_ROW_H - font * 1.25) * 0.5,
                ),
                font,
                color,
            )
        });
        let Ui { ctx, text, .. } = self;
        text.add_text(
            &mut ctx.paint,
            label,
            style.fonts.body,
            text_pos,
            label_color,
            None,
        );
        if let Some((pos, font, color)) = shortcut_draw {
            let shortcut = shortcut.unwrap_or_default();
            text.add_text_family(
                &mut ctx.paint,
                shortcut,
                font,
                pos,
                color,
                None,
                crate::text::FontFamily::Mono,
            );
        }
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
        RectShape::fill(
            panel,
            style.rounding.panel,
            style.palette.elevated.with_alpha(style.palette.popover_alpha),
        )
        .with_stroke(style.metrics.border, style.palette.stroke_strong),
    ));
    ui.ctx_mut().overlay_paint.extend(drained);
    ui.ctx_mut().overlay_paint.extend(children);

    let area = ui.ctx_mut().memory.area_entry(area_id, panel.min, Order::Tooltip);
    area.pos = panel.min;
    area.size = panel.size();

    // Rule 1: outside-press / Escape / wheel / focus-loss are resolved once in
    // `Context::begin_frame` and the offending input is consumed there. The
    // registered rect spans the trigger so the menu button's own press still
    // reaches its toggle; nested flyouts and combo dropdowns push themselves
    // *above* this panel on the stack, which is what keeps a click inside a
    // child from collapsing the chain (this replaces `topmost_area_contains`).
    ui.ctx_mut().modal_push(area_id, trigger.union(panel));

    let closed = ui.ctx().memory.popup_close_requested
        || ui.ctx().modal_dismissed(area_id).is_some();
    if closed {
        ui.ctx_mut().modal_dismiss(area_id);
    }
    closed
}
