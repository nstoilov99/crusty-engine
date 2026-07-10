use std::ops::RangeInclusive;

use crate::animation::approach_half_life;
use crate::context::{CursorIcon, Ui};
use crate::math::{Color, Pos2, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::widgets::TextEdit;

/// Persisted while the pointer drags the field horizontally.
#[derive(Clone, Copy)]
struct DragState {
    last_x: f32,
    total_dx: f32,
}

/// Persisted while the field is in click-to-type edit mode.
struct EditState {
    text: String,
    just_opened: bool,
}

/// Numeric field in the spirit of egui's `DragValue`: drag horizontally to
/// scrub the value, click (without dragging) to type it. Styled like the
/// rest of crusty (flat fill, 1px stroke, accent on interaction).
pub struct DragValue<'a> {
    value: &'a mut f32,
    // f64 like egui: literal 0.01 gives 2 auto decimals, while an
    // f32-widened 0.01 gives 3 — call sites rely on that distinction.
    speed: f64,
    range: Option<RangeInclusive<f32>>,
    prefix: String,
    suffix: String,
    min_decimals: Option<usize>,
    max_decimals: Option<usize>,
    width: Option<f32>,
    height: Option<f32>,
    fill: Option<Color>,
}

impl<'a> DragValue<'a> {
    pub fn new(value: &'a mut f32) -> Self {
        Self {
            value,
            speed: 0.1,
            range: None,
            prefix: String::new(),
            suffix: String::new(),
            min_decimals: None,
            max_decimals: None,
            width: None,
            height: None,
            fill: None,
        }
    }

    /// Display-only prefix shown before the number (e.g. `"X: "`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Value change per dragged pixel (default 0.1).
    pub fn speed(mut self, speed: impl Into<f64>) -> Self {
        self.speed = speed.into();
        self
    }

    /// Clamp the value to this range on drag and on typed input.
    pub fn range(mut self, range: RangeInclusive<f32>) -> Self {
        self.range = Some(range);
        self
    }

    /// Display-only suffix appended after the number (e.g. `"°"`).
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Maximum shown decimal places. Default: speed-derived (like egui) —
    /// `ceil(log10(1/speed))` plus two extra digits when needed to
    /// round-trip the value.
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.max_decimals = Some(decimals);
        self
    }

    /// Minimum shown decimal places (trailing zeros kept up to this count).
    /// Default: speed-derived, `ceil(log10(1/speed))` clamped to [0, 15].
    pub fn min_decimals(mut self, decimals: usize) -> Self {
        self.min_decimals = Some(decimals);
        self
    }

    fn decimal_range(&self) -> (usize, usize) {
        let auto = if self.speed.abs() > 0.0 {
            (1.0 / self.speed.abs()).log10().ceil().clamp(0.0, 15.0) as usize
        } else {
            2
        };
        let min = self.min_decimals.unwrap_or(0);
        let max = self.max_decimals.unwrap_or(auto + 2).max(min);
        (auto.clamp(min, max), max)
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    /// Override the field background (default: `palette.surface_active`).
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    fn clamp(&self, v: f32) -> f32 {
        match &self.range {
            Some(r) => v.clamp(*r.start(), *r.end()),
            None => v,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id("drag_value");
        let font = style.fonts.body;
        let width = self.width.unwrap_or(64.0);
        let height = self.height.unwrap_or(font + 6.0);
        let (min_dec, max_dec) = self.decimal_range();

        // ── Edit mode: hand the rect to an embedded TextEdit until the
        //    user commits (Enter / click-away) or cancels (Escape).
        if let Some(mut edit) = ui.ctx_mut().memory.data_remove::<EditState>(id) {
            let out = TextEdit::new(&mut edit.text)
                .width(width)
                .height(height)
                .fill(self.fill.unwrap_or(style.palette.surface_active))
                .request_focus(edit.just_opened)
                .show_full(ui);
            edit.just_opened = false;
            let commit = out.submitted || (!out.focused && !out.cancelled);
            if commit {
                let raw = edit.text.trim();
                let raw = raw.strip_prefix(self.prefix.trim()).unwrap_or(raw);
                if let Ok(v) = raw.trim_end_matches(&self.suffix).trim().parse::<f32>() {
                    *self.value = self.clamp(v);
                }
            }
            if !commit && !out.cancelled {
                ui.ctx_mut().memory.data_insert(id, edit);
            }
            return out.response;
        }

        // ── Drag mode.
        let rect = ui.allocate(Vec2::new(width, height));
        let hovered = ui.contains_pointer(rect);
        let pointer_pos = ui.ctx().input.pointer_pos;
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_released = ui.ctx().input.pointer_released;
        let pointer_down = ui.ctx().input.pointer_down;

        if pointer_pressed && hovered {
            ui.ctx_mut().memory.active_widget = Some(id);
            if let Some(p) = pointer_pos {
                ui.ctx_mut()
                    .memory
                    .data_insert(id, DragState { last_x: p.x, total_dx: 0.0 });
            }
        }
        let is_active = ui.ctx().memory.active_widget == Some(id);
        let pressed = is_active && pointer_down;

        // Scrub: past a small dead zone, every dragged pixel changes the
        // value by `speed`. Below it a press-release still counts as a
        // click that opens edit mode.
        const DRAG_DEAD_ZONE: f32 = 3.0;
        let mut scrubbing = false;
        if pressed
            && let Some(p) = pointer_pos
        {
            let mut delta = 0.0;
            if let Some(st) = ui.ctx_mut().memory.data_get_mut::<DragState>(id) {
                let dx = p.x - st.last_x;
                st.last_x = p.x;
                st.total_dx += dx;
                if st.total_dx.abs() > DRAG_DEAD_ZONE {
                    delta = dx;
                    scrubbing = true;
                }
            }
            if delta != 0.0 {
                *self.value = self.clamp(*self.value + (delta as f64 * self.speed) as f32);
            }
        }

        let mut clicked = false;
        if pointer_released && is_active {
            let was_click = ui
                .ctx()
                .memory
                .data_get::<DragState>(id)
                .is_none_or(|st| st.total_dx.abs() <= DRAG_DEAD_ZONE);
            ui.ctx_mut().memory.data_remove::<DragState>(id);
            if was_click && hovered {
                clicked = true;
                ui.ctx_mut().memory.data_insert(
                    id,
                    EditState {
                        text: format_value(*self.value, min_dec, max_dec, ""),
                        just_opened: true,
                    },
                );
            }
        }

        if scrubbing || hovered {
            ui.ctx_mut().set_cursor_icon(CursorIcon::ResizeEw);
        }

        let dt = ui.ctx().dt();
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim = approach_half_life(
            mem.hover_anim,
            if hovered || pressed { 1.0 } else { 0.0 },
            dt,
            style.motion.hover_half_life,
        );
        mem.press_anim = approach_half_life(
            mem.press_anim,
            if pressed { 1.0 } else { 0.0 },
            dt,
            style.motion.press_half_life,
        );
        let hover_anim = mem.hover_anim;
        let press_anim = mem.press_anim;

        let fill = self.fill.unwrap_or(style.palette.surface_active);
        let stroke = lerp_color(style.palette.stroke, style.palette.accent, hover_anim);
        let shape = RectShape::fill(rect, Rounding::same(3.0), fill).with_stroke(1.0, stroke);
        ui.ctx_mut().paint.push(PaintCmd::Rrect(shape));

        let text = format!(
            "{}{}",
            self.prefix,
            format_value(*self.value, min_dec, max_dec, &self.suffix)
        );
        let text_size = ui.text_mut().measure(&text, font, None);
        let text_pos = Pos2::new(
            rect.min.x + (rect.width() - text_size.x) * 0.5,
            rect.min.y + (rect.height() - text_size.y) * 0.5,
        );
        let color = style.palette.text;
        let Ui { ctx, text: tr, .. } = ui;
        tr.add_text(&mut ctx.paint, &text, font, text_pos, color, None);

        Response {
            id,
            rect,
            hovered,
            pressed,
            clicked,
            hover_anim,
            press_anim,
        }
    }
}

// Fewest decimals in [min, max] that round-trips the value, else max
// (egui's `format_with_decimals_in_range` behavior).
fn format_value(v: f32, min_decimals: usize, max_decimals: usize, suffix: &str) -> String {
    let mut s = None;
    for d in min_decimals..max_decimals {
        let t = format!("{v:.d$}");
        if t.parse::<f32>().is_ok_and(|p| p == v) {
            s = Some(t);
            break;
        }
    }
    let mut s = s.unwrap_or_else(|| format!("{v:.max_decimals$}"));
    if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
        s = s.trim_start_matches('-').to_string();
    }
    s.push_str(suffix);
    s
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}
