use crate::animation::approach_half_life;
use crate::context::{Direction, Ui, UiOptions};
use crate::id::Id;
use crate::input::Key;
use crate::layer::Order;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};

/// A floating window: movable by its title bar, optionally resizable
/// (bottom-right grip), collapsible (chevron) and closable (✕ writes
/// `false` through the `open` handle).
///
/// Windows paint into their own layer and are z-sorted: clicking
/// anywhere on a window brings it to the front, and widgets in occluded
/// windows ignore the pointer.
///
/// `modal(true)` lifts the window into [`Order::Modal`]: it dims the
/// rest of the screen and blocks all interaction outside itself.
/// Combine with [`close_on_click_outside`](Self::close_on_click_outside)
/// for dismiss-on-backdrop-click dialogs; Esc always closes a modal that
/// has an `open` handle.
pub struct Window<'open> {
    title: String,
    open: Option<&'open mut bool>,
    default_pos: Pos2,
    default_size: Vec2,
    resizable: bool,
    collapsible: bool,
    modal: bool,
    close_on_click_outside: bool,
    auto_size: bool,
}

/// What [`Window::show`] did this frame.
pub struct WindowResponse<R> {
    /// Outer rect (title bar + visible body) this frame.
    pub rect: Rect,
    /// Body closure result — `None` while the window is collapsed.
    pub inner: Option<R>,
}

impl<'open> Window<'open> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            open: None,
            default_pos: Pos2::new(160.0, 160.0),
            default_size: Vec2::new(320.0, 260.0),
            resizable: true,
            collapsible: true,
            modal: false,
            close_on_click_outside: false,
            auto_size: false,
        }
    }

    /// Show a ✕ button that writes `false` through the handle; the
    /// window is skipped entirely while `*open == false`.
    pub fn open(mut self, open: &'open mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn default_pos(mut self, pos: Pos2) -> Self {
        self.default_pos = pos;
        self
    }

    pub fn default_size(mut self, size: Vec2) -> Self {
        self.default_size = size;
        self
    }

    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    /// Fit the window height to its content every frame (measured with a
    /// one-frame lag, like all immediate-mode geometry). Width still comes
    /// from [`default_size`](Self::default_size). Overrides manual resizing
    /// of the height, so it is usually paired with `resizable(false)`.
    pub fn auto_size(mut self, v: bool) -> Self {
        self.auto_size = v;
        self
    }

    pub fn collapsible(mut self, v: bool) -> Self {
        self.collapsible = v;
        self
    }

    /// Render above everything non-popup, dim the backdrop and block
    /// interaction with the UI beneath.
    pub fn modal(mut self, v: bool) -> Self {
        self.modal = v;
        self
    }

    /// Close (requires an [`open`](Self::open) handle) when the user
    /// presses the pointer outside the window.
    pub fn close_on_click_outside(mut self, v: bool) -> Self {
        self.close_on_click_outside = v;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<WindowResponse<R>> {
        if let Some(open) = &self.open {
            if !**open {
                return None;
            }
        }

        let style = ui.style();
        let id = Id::new(("window", &self.title));
        let order = if self.modal {
            Order::Modal
        } else {
            Order::Middle
        };
        let title_h = style.fonts.body + 14.0;
        let min_size = Vec2::new(140.0, title_h + 60.0);
        let screen = ui.ctx().screen_rect();

        // ── Persistent state
        let (mut pos, mut size) = {
            let area = ui
                .ctx_mut()
                .memory
                .area_entry(id, self.default_pos, order);
            if area.size == Vec2::ZERO {
                area.size = self.default_size;
            }
            (area.pos, area.size)
        };
        size = size.max(min_size);

        let state_id = id.with("state");
        let first_time = ui.ctx().memory.get(state_id).is_none();
        if first_time {
            ui.ctx_mut().memory.get_or_default(state_id).aux_anim = 1.0;
        }
        let mut collapsed = ui
            .ctx()
            .memory
            .get(state_id)
            .map(|m| m.bool_state)
            .unwrap_or(false);
        let openness = ui
            .ctx()
            .memory
            .get(state_id)
            .map(|m| m.aux_anim)
            .unwrap_or(1.0);

        // ── Interaction (uses last-frame geometry, standard IM pattern)
        let pointer_pos = ui.ctx().input.pointer_pos;
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_down = ui.ctx().input.pointer_down;
        let on_window = ui.ctx().hovered_area() == Some(id);

        if pointer_pressed && on_window {
            ui.ctx_mut().memory.bring_area_to_front(id);
        }

        let body_h = ((size.y - title_h) * openness).max(0.0);
        let outer_h = title_h + body_h;
        let rect = Rect::from_min_size(pos, Vec2::new(size.x, outer_h));
        let title_rect = Rect::from_min_size(pos, Vec2::new(size.x, title_h));

        let btn_size = title_h - 10.0;
        let close_rect = Rect::from_center_size(
            Pos2::new(title_rect.max.x - title_h * 0.5, title_rect.center().y),
            Vec2::splat(btn_size),
        );
        let chev_rect = Rect::from_center_size(
            Pos2::new(title_rect.min.x + title_h * 0.5, title_rect.center().y),
            Vec2::splat(btn_size),
        );
        let grip = 14.0;
        let grip_rect = Rect::from_min_max(
            Pos2::new(rect.max.x - grip, rect.max.y - grip),
            rect.max,
        );

        let drag_id = id.with("drag");
        let resize_id = id.with("resize");
        let active = ui.ctx().memory.active_widget;

        // Resize via bottom-right grip.
        let can_resize = self.resizable && !collapsed;
        if can_resize && pointer_pressed && on_window {
            if let Some(p) = pointer_pos {
                if grip_rect.contains(p) {
                    ui.ctx_mut().memory.active_widget = Some(resize_id);
                    let mem = ui.ctx_mut().memory.get_or_default(resize_id);
                    mem.aux_vec2 = rect.max - p;
                }
            }
        }
        let resizing = ui.ctx().memory.active_widget == Some(resize_id) && pointer_down;
        if resizing {
            if let Some(p) = pointer_pos {
                let grab = ui
                    .ctx()
                    .memory
                    .get(resize_id)
                    .map(|m| m.aux_vec2)
                    .unwrap_or(Vec2::ZERO);
                size = ((p + grab) - pos).max(min_size);
            }
        }

        // Move via title bar (excluding the chevron / close buttons).
        if pointer_pressed && on_window && active != Some(resize_id) && !resizing {
            if let Some(p) = pointer_pos {
                let on_buttons = (self.collapsible && chev_rect.contains(p))
                    || (self.open.is_some() && close_rect.contains(p));
                if title_rect.contains(p) && !on_buttons {
                    ui.ctx_mut().memory.active_widget = Some(drag_id);
                    let mem = ui.ctx_mut().memory.get_or_default(drag_id);
                    mem.aux_vec2 = p - pos;
                }
            }
        }
        if ui.ctx().memory.active_widget == Some(drag_id) && pointer_down {
            if let Some(p) = pointer_pos {
                let grab = ui
                    .ctx()
                    .memory
                    .get(drag_id)
                    .map(|m| m.aux_vec2)
                    .unwrap_or(Vec2::ZERO);
                pos = p - grab;
            }
        }

        // Keep the whole window inside the screen.
        if screen.width() > 0.0 {
            pos.x = pos.x.clamp(screen.min.x, (screen.max.x - size.x).max(screen.min.x));
            pos.y = pos.y.clamp(screen.min.y, (screen.max.y - size.y).max(screen.min.y));
        }

        // Collapse toggle.
        let mut chev_hovered = false;
        if self.collapsible {
            chev_hovered =
                on_window && pointer_pos.map_or(false, |p| chev_rect.contains(p));
            if pointer_pressed && chev_hovered {
                collapsed = !collapsed;
            }
        }

        // Close button / Esc / click-outside.
        let mut close_hovered = false;
        let mut close_requested = false;
        if self.open.is_some() {
            close_hovered =
                on_window && pointer_pos.map_or(false, |p| close_rect.contains(p));
            if pointer_pressed && close_hovered {
                close_requested = true;
            }
            if self.modal && ui.ctx().input.key_pressed(Key::Escape) {
                close_requested = true;
            }
            if self.close_on_click_outside
                && pointer_pressed
                && pointer_pos.map_or(false, |p| !rect.contains(p))
            {
                close_requested = true;
            }
        }

        // ── Recompute visible rect with post-interaction geometry.
        let dt = ui.ctx().dt();
        let openness = approach_half_life(
            openness,
            if collapsed { 0.0 } else { 1.0 },
            dt,
            style.motion.state_half_life,
        );
        let body_h = ((size.y - title_h) * openness).max(0.0);
        let outer_h = title_h + body_h;
        let rect = Rect::from_min_size(pos, Vec2::new(size.x, outer_h));
        let title_rect = Rect::from_min_size(pos, Vec2::new(size.x, title_h));
        let body_rect = Rect::from_min_max(
            Pos2::new(rect.min.x, title_rect.max.y),
            rect.max,
        );
        // Re-derive title-bar element rects from the post-interaction
        // geometry so they track the frame instead of lagging a frame
        // behind it while dragging.
        let close_rect = Rect::from_center_size(
            Pos2::new(title_rect.max.x - title_h * 0.5, title_rect.center().y),
            Vec2::splat(btn_size),
        );
        let chev_rect = Rect::from_center_size(
            Pos2::new(title_rect.min.x + title_h * 0.5, title_rect.center().y),
            Vec2::splat(btn_size),
        );
        let is_front = ui.ctx().memory.is_area_front(id);

        // ── Paint + body, in this window's own layer.
        ui.ctx_mut().begin_layer();

        if self.modal {
            ui.ctx_mut().paint.push(PaintCmd::Rect {
                rect: screen,
                color: Color::rgba(0.0, 0.0, 0.0, 0.45),
            });
        }

        let stroke_color = if is_front {
            style.palette.stroke_hover
        } else {
            style.palette.stroke
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, style.rounding.panel, style.palette.surface)
                .with_stroke(1.0, stroke_color)
                .with_shadow(if is_front { 30.0 } else { 18.0 }, Color::rgba(0.0, 0.0, 0.0, 0.5)),
        ));

        // Title bar: round only the top corners (bottom ones too while
        // collapsed, so the bar doesn't poke out of the rounded frame).
        let bar_round = Rounding {
            nw: style.rounding.panel.nw,
            ne: style.rounding.panel.ne,
            sw: style.rounding.panel.sw * (1.0 - openness),
            se: style.rounding.panel.se * (1.0 - openness),
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
            title_rect,
            bar_round,
            style.palette.surface_active,
        )));

        // Chevron (collapse) + title + close.
        let title_text_x = if self.collapsible {
            chev_rect.max.x + 6.0
        } else {
            title_rect.min.x + 10.0
        };
        {
            let Ui { ctx, text, .. } = ui;
            if self.collapsible {
                let chev_color = if chev_hovered {
                    style.palette.accent
                } else {
                    style.palette.text_dim
                };
                crate::widgets::paint_chevron(
                    &mut ctx.paint,
                    chev_rect.center(),
                    style.fonts.small * 0.32,
                    (style.fonts.small * 0.13).max(1.5),
                    !collapsed,
                    chev_color,
                );
            }
            let title_size = text.measure(&self.title, style.fonts.body, None);
            text.add_text(
                &mut ctx.paint,
                &self.title,
                style.fonts.body,
                Pos2::new(
                    title_text_x,
                    title_rect.center().y - title_size.y * 0.5,
                ),
                style.palette.text,
                None,
            );
            if self.open.is_some() {
                let cross = "\u{2715}";
                let cross_size = text.measure(cross, style.fonts.small, None);
                let cross_color = if close_hovered {
                    style.palette.accent
                } else {
                    style.palette.text_dim
                };
                text.add_text(
                    &mut ctx.paint,
                    cross,
                    style.fonts.small,
                    Pos2::new(
                        close_rect.center().x - cross_size.x * 0.5,
                        close_rect.center().y - cross_size.y * 0.5,
                    ),
                    cross_color,
                    None,
                );
            }
        }

        // Body (clipped so mid-collapse and undersized windows truncate
        // cleanly instead of spilling).
        let mut inner = None;
        let mut content_bottom = None;
        if body_h > 1.0 {
            ui.ctx_mut().paint.push(PaintCmd::PushClip(body_rect));
            let opts = UiOptions {
                padding: Vec2::splat(style.spacing.padding),
                spacing: style.spacing.item,
            };
            let Ui { ctx, text, .. } = ui;
            let mut body_ui = Ui::with_root(
                ctx,
                text,
                body_rect,
                Direction::TopDown,
                opts,
                id.with("body"),
            );
            body_ui.set_area(Some(id));
            inner = Some(body(&mut body_ui));
            content_bottom = Some(body_ui.cursor().y);
            ui.ctx_mut().paint.push(PaintCmd::PopClip);
        }

        // Fit height to the measured content (applies next frame).
        if let (true, Some(bottom)) = (self.auto_size, content_bottom) {
            let content_h =
                (bottom - body_rect.min.y) - style.spacing.item + style.spacing.padding;
            size.y = (title_h + content_h.max(0.0))
                .max(min_size.y)
                .min(screen.height().max(min_size.y));
        }

        // Resize grip.
        if can_resize {
            let grip_hovered =
                on_window && pointer_pos.map_or(false, |p| grip_rect.contains(p));
            let grip_color = if resizing || grip_hovered {
                style.palette.accent
            } else {
                style.palette.text_dim
            };
            // Three steps of a diagonal "staircase" marker.
            for i in 0..3 {
                let inset = 3.0 + i as f32 * 4.0;
                let dot = Rect::from_min_size(
                    Pos2::new(rect.max.x - inset - 2.0, rect.max.y - inset - 2.0),
                    Vec2::splat(2.0),
                );
                ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                    dot,
                    Rounding::same(1.0),
                    grip_color,
                )));
            }
        }

        ui.ctx_mut().end_layer(id, order);

        // ── Persist
        {
            let area = ui.ctx_mut().memory.area_entry(id, self.default_pos, order);
            area.pos = pos;
            area.size = size;
        }
        {
            let mem = ui.ctx_mut().memory.get_or_default(state_id);
            mem.seen_this_frame = true;
            mem.last_rect = rect;
            mem.bool_state = collapsed;
            mem.aux_anim = openness;
        }
        // Keep drag/resize scratch memories alive while held.
        for scratch in [drag_id, resize_id] {
            let mem = ui.ctx_mut().memory.get_or_default(scratch);
            mem.seen_this_frame = true;
        }

        if close_requested {
            if let Some(open) = self.open {
                *open = false;
            }
        }

        Some(WindowResponse { rect, inner })
    }
}
