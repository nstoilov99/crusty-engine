use crate::context::{Direction, Ui, UiOptions};
use crate::layer::Order;
use crate::math::{Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// Right-click `rect` to open a floating menu at the pointer position;
/// the menu body is provided by the closure. Click outside or Esc closes
/// it. Selecting a `SelectableValue` inside the menu auto-closes via
/// `Memory::popup_close_requested` (same mechanism ComboBox uses).
///
/// Phase 8 ships the simplest version: one menu can be open at a time,
/// keyed by `rect`'s id_source.
pub fn context_menu_for(
    ui: &mut Ui,
    id_source: impl std::hash::Hash,
    rect: Rect,
    body: impl FnOnce(&mut Ui),
) {
    let right_pressed = ui.ctx().input.right_pressed;
    let pointer_in_rect = ui.contains_pointer(rect);
    let open_at = if right_pressed && pointer_in_rect {
        ui.ctx().input.pointer_pos
    } else {
        // A right press that dismissed some *other* transient surface is
        // consumed before widgets see it (Rule 1), but its position is handed
        // on precisely so the click can still land here — dismiss and reopen in
        // one gesture rather than making the user click twice.
        match ui.ctx().modal_reopen_at().filter(|p| rect.contains(*p)) {
            Some(p) => {
                ui.ctx_mut().take_modal_reopen();
                Some(p)
            }
            None => None,
        }
    };
    context_menu_at(ui, id_source, open_at, body)
}

/// Programmatically-opened variant of [`context_menu_for`]: the host detects
/// the right-click itself (e.g. the dock reporting a tab right-click) and
/// passes the pointer position on that frame. Same close semantics.
pub fn context_menu_at(
    ui: &mut Ui,
    id_source: impl std::hash::Hash,
    open_at: Option<Pos2>,
    body: impl FnOnce(&mut Ui),
) {
    let style = ui.style();
    let id = ui.alloc_id(("context_menu", &id_source));

    let menu_area_id = id.with("menu_area");

    let (mut is_open, mut anchor) = match ui.ctx().memory.get(id) {
        Some(m) => (m.bool_state, m.aux_vec2),
        None => (false, Vec2::ZERO),
    };

    // Rule 1: outside-press / Escape / wheel-outside / focus-loss dismissal is
    // resolved once, in `Context::begin_frame`, against last frame's rect — and
    // the input that did it has already been consumed, so nothing underneath
    // this menu will also act on it.
    if ui.ctx().modal_dismissed(menu_area_id).is_some() {
        is_open = false;
    }

    if let Some(p) = open_at {
        is_open = true;
        anchor = Vec2::new(p.x, p.y);
    }

    let mut menu_rect = Rect::ZERO;
    if is_open {
        let menu_root_id = id.with("menu");
        let anchor_rect =
            Rect::from_min_size(Pos2::new(anchor.x, anchor.y), Vec2::ZERO);
        let menu_pos =
            super::popup_pos(ui, menu_area_id, anchor_rect, 180.0, 20.0, 0.0);
        let scratch_rect =
            Rect::from_min_size(menu_pos, Vec2::new(180.0, 600.0));

        let body_start = ui.ctx().paint.len();
        let used_h = {
            let Ui { ctx, text, .. } = ui;
            let opts = UiOptions {
                padding: Vec2::new(4.0, 4.0),
                spacing: 2.0,
            };
            let mut inner = Ui::with_root(
                ctx,
                text,
                scratch_rect,
                Direction::TopDown,
                opts,
                menu_root_id,
            );
            inner.set_area(Some(menu_area_id));
            body(&mut inner);
            inner.used_rect().height()
        };

        let drained: Vec<PaintCmd> =
            ui.ctx_mut().paint.drain(body_start..).collect();

        menu_rect = Rect::from_min_size(
            menu_pos,
            Vec2::new(180.0, (used_h + 8.0).max(20.0)),
        );

        ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
            RectShape::fill(
                menu_rect,
                style.rounding.panel,
                style.palette.elevated.with_alpha(style.palette.popover_alpha),
            )
            .with_stroke(style.metrics.border, style.palette.stroke_strong),
        ));
        ui.ctx_mut().overlay_paint.extend(drained);

        let area = ui.ctx_mut().memory.area_entry(
            menu_area_id,
            menu_rect.min,
            Order::Tooltip,
        );
        area.pos = menu_rect.min;
        area.size = menu_rect.size();

        // Register for next frame's dismissal check, and for the ordering that
        // lets a nested popup shield this menu from the outside-press rule.
        ui.ctx_mut().modal_push(menu_area_id, menu_rect);

        // A menu item ran a command: take the menu down with it.
        if ui.ctx().memory.popup_close_requested {
            is_open = false;
        }
    }

    if !is_open {
        ui.ctx_mut().modal_dismiss(menu_area_id);
    }

    let _ = menu_rect;
    let mem = ui.ctx_mut().memory.get_or_default(id);
    mem.seen_this_frame = true;
    mem.bool_state = is_open;
    mem.aux_vec2 = anchor;
}
