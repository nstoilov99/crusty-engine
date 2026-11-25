use crate::animation::approach_half_life;
use crate::clipboard;
use crate::context::{CursorIcon, Ui};
use crate::input::{Key, Modifiers};
use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::text::TextRenderer;

/// Cursor + selection state, persisted in the typed memory store. Both are
/// byte offsets into the value string and always sit on a UTF-8 boundary.
/// When `cursor == anchor` there is no selection.
#[derive(Clone, Copy, Default)]
struct TextEditState {
    cursor: usize,
    anchor: usize,
    /// Horizontal scroll applied when the value is wider than the field, in
    /// points. Persisted only so a *click* can be mapped back to a byte index
    /// on the next frame — it is recomputed from the caret every frame.
    scroll: f32,
}

/// Result of [`TextEdit::show_full`]: the plain [`Response`] plus the
/// keyboard events a caller like a console or search box needs to react to.
pub struct TextEditOutput {
    pub response: Response,
    /// Enter was pressed this frame while focused.
    pub submitted: bool,
    /// Escape was pressed this frame while focused (focus was dropped).
    pub cancelled: bool,
    /// ArrowUp was pressed this frame while focused (e.g. history back).
    pub nav_up: bool,
    /// ArrowDown was pressed this frame while focused (e.g. history forward).
    pub nav_down: bool,
    /// The field holds keyboard focus after this frame's processing.
    pub focused: bool,
}

/// Single-line text input.
///
/// Supports click-to-place and drag-to-select cursor, arrow / Home / End
/// navigation (Shift extends the selection), Backspace / Delete, select-all
/// (Ctrl+A), and system clipboard cut / copy / paste (Ctrl+X / C / V).
pub struct TextEdit<'a> {
    value: &'a mut String,
    width: Option<f32>,
    height: Option<f32>,
    hint: Option<String>,
    keep_focus_on_submit: bool,
    fill: Option<Color>,
    request_focus: bool,
}

impl<'a> TextEdit<'a> {
    pub fn new(value: &'a mut String) -> Self {
        Self {
            value,
            width: None,
            height: None,
            hint: None,
            keep_focus_on_submit: false,
            fill: None,
            request_focus: false,
        }
    }

    /// Override the field background (default: `palette.active`).
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Override the field height (default: body font size + 10).
    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    /// Placeholder text shown (dimmed) when the value is empty.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Keep keyboard focus after Enter (a console input that submits
    /// repeatedly). Default: Enter drops focus like Escape.
    pub fn keep_focus_on_submit(mut self, v: bool) -> Self {
        self.keep_focus_on_submit = v;
        self
    }

    /// Grab keyboard focus this frame (e.g. an inline rename field on its
    /// first frame). Pass true once — passing it every frame would keep
    /// re-grabbing focus and make it impossible to click away.
    pub fn request_focus(mut self, v: bool) -> Self {
        self.request_focus = v;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_full(ui).response
    }

    /// Like [`show`](Self::show), but also reports Enter / Escape /
    /// ArrowUp / ArrowDown so callers can implement submit + history.
    pub fn show_full(self, ui: &mut Ui) -> TextEditOutput {
        let style = ui.style();
        let id = ui.alloc_id("text_edit");
        let font = style.fonts.body;
        let width = self
            .width
            .unwrap_or_else(|| ui.available().width().min(220.0));
        let height = self.height.unwrap_or(font + 10.0);
        let rect = ui.allocate(Vec2::new(width, height));
        let text_pad = 6.0;
        let text_x = rect.min.x + text_pad;
        let scope = ui.area();
        ui.ctx_mut().memory.register_focusable(id, scope);
        if self.request_focus {
            ui.ctx_mut().memory.focused_widget = Some(id);
        }

        // ── Snapshot input up front to keep borrows simple.
        let hovered = ui.contains_pointer(rect);
        if hovered {
            ui.ctx_mut().set_cursor_icon(CursorIcon::Text);
        }
        let pointer_pos = ui.ctx().input.pointer_pos;
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_down = ui.ctx().input.pointer_down;
        let pointer_released = ui.ctx().input.pointer_released;
        let mods = ui.ctx().input.modifiers;
        let active = ui.ctx().memory.active_widget;

        // Map a pointer x to a cursor byte index when starting a click or
        // dragging a selection (needs the text measurer).
        let start_click = pointer_pressed && hovered;
        let dragging = active == Some(id) && pointer_down && !pointer_pressed;
        // Hit-test against where the glyphs actually were: a scrolled field
        // draws its text left of `text_x`.
        let prev_scroll = ui
            .ctx()
            .memory
            .data_get::<TextEditState>(id)
            .map_or(0.0, |s| s.scroll);
        let click_idx = if start_click || dragging {
            pointer_pos
                .map(|p| byte_index_at_x(ui.text_mut(), self.value, font, text_x - prev_scroll, p.x))
        } else {
            None
        };

        if start_click {
            ui.ctx_mut().memory.focused_widget = Some(id);
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let focused = ui.ctx().memory.focused_widget == Some(id);
        if focused {
            // Tells the host "the user is typing" so single-key shortcuts hold.
            ui.ctx_mut().memory.text_focus = true;
        }
        let mut clicked = start_click;
        let mut submitted = false;
        let mut cancelled = false;
        let mut nav_up = false;
        let mut nav_down = false;

        // ── Load persisted cursor/selection, clamped to the live string
        //    (the value may have been edited by the app between frames).
        let mut st = ui
            .ctx()
            .memory
            .data_get::<TextEditState>(id)
            .copied()
            .unwrap_or_default();
        st.cursor = clamp_boundary(self.value, st.cursor);
        st.anchor = clamp_boundary(self.value, st.anchor);

        // Programmatic focus grab selects everything (rename fields,
        // DragValue click-to-type) so typing replaces the old value.
        if self.request_focus {
            st.anchor = 0;
            st.cursor = self.value.len();
        }

        if let Some(ci) = click_idx {
            st.cursor = ci;
            // A plain click collapses the selection; Shift+click and drags
            // keep the anchor so the selection grows.
            if start_click && !mods.contains(Modifiers::SHIFT) {
                st.anchor = ci;
            }
        }

        if focused {
            // Insert composed text, unless a Ctrl/Cmd chord is active (those
            // are shortcuts, not text). Always drain the shared pool so it
            // doesn't leak into other widgets.
            let chord = mods.contains(Modifiers::CTRL) || mods.contains(Modifiers::META);
            let typed = std::mem::take(&mut ui.ctx_mut().input.typed_text);
            if !chord {
                let filtered: String = typed.chars().filter(|c| !c.is_control()).collect();
                if !filtered.is_empty() {
                    replace_selection(self.value, &mut st, &filtered);
                }
            }

            let key_presses = ui.ctx().input.key_presses.clone();
            for press in key_presses {
                let shift = press.modifiers.contains(Modifiers::SHIFT);
                let ctrl = press.modifiers.contains(Modifiers::CTRL)
                    || press.modifiers.contains(Modifiers::META);
                match press.key {
                    Key::ArrowLeft => move_left(self.value, &mut st, shift),
                    Key::ArrowRight => move_right(self.value, &mut st, shift),
                    Key::Home => {
                        st.cursor = 0;
                        if !shift {
                            st.anchor = 0;
                        }
                    }
                    Key::End => {
                        st.cursor = self.value.len();
                        if !shift {
                            st.anchor = st.cursor;
                        }
                    }
                    Key::Backspace => backspace(self.value, &mut st),
                    Key::Delete => delete_forward(self.value, &mut st),
                    Key::Char('a' | 'A') if ctrl => {
                        st.anchor = 0;
                        st.cursor = self.value.len();
                    }
                    Key::Char('c' | 'C') if ctrl => {
                        if let Some(sel) = selected(self.value, &st) {
                            clipboard::set_text(sel);
                        }
                    }
                    Key::Char('x' | 'X') if ctrl => {
                        if let Some(sel) = selected(self.value, &st) {
                            clipboard::set_text(sel);
                            delete_selection(self.value, &mut st);
                        }
                    }
                    Key::Char('v' | 'V') if ctrl => {
                        if let Some(t) = clipboard::get_text() {
                            let one_line: String =
                                t.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                            replace_selection(self.value, &mut st, &one_line);
                        }
                    }
                    Key::Enter => {
                        submitted = true;
                        if !self.keep_focus_on_submit {
                            ui.ctx_mut().memory.focused_widget = None;
                        }
                        clicked = true;
                    }
                    Key::Escape => {
                        cancelled = true;
                        ui.ctx_mut().memory.focused_widget = None;
                        clicked = true;
                    }
                    // Report vertical arrows (history nav) and park the
                    // caret at "end of string": the caller usually replaces
                    // the value, and next frame's clamp lands the cursor at
                    // the end of the new text.
                    Key::ArrowUp => {
                        nav_up = true;
                        st.cursor = usize::MAX;
                        st.anchor = usize::MAX;
                    }
                    Key::ArrowDown => {
                        nav_down = true;
                        st.cursor = usize::MAX;
                        st.anchor = usize::MAX;
                    }
                    _ => {}
                }
            }
        }

        if pointer_released && active == Some(id) {
            ui.ctx_mut().memory.active_widget = None;
        }

        // Persist cursor/selection for next frame. The persisted copy may
        // hold the usize::MAX "end of string" sentinel from history nav;
        // re-clamp the local copy before it's used to slice for measuring.
        ui.ctx_mut().memory.data_insert::<TextEditState>(id, st);
        st.cursor = clamp_boundary(self.value, st.cursor);
        st.anchor = clamp_boundary(self.value, st.anchor);

        // ── Measure the x positions we need to draw (caret + selection).
        let (sel_a, sel_b) = sel_range(&st);
        let caret_off = measure_prefix(ui.text_mut(), self.value, font, st.cursor);
        let sel_a_off = if sel_a == st.cursor {
            caret_off
        } else {
            measure_prefix(ui.text_mut(), self.value, font, sel_a)
        };
        let sel_b_off = if sel_b == st.cursor {
            caret_off
        } else {
            measure_prefix(ui.text_mut(), self.value, font, sel_b)
        };

        // ── Animation state.
        let dt = ui.ctx().dt();
        let target_hover = if hovered { 1.0 } else { 0.0 };
        let target_focus = if focused { 1.0 } else { 0.0 };
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = rect;
        mem.hover_anim =
            approach_half_life(mem.hover_anim, target_hover, dt, style.motion.hover_half_life);
        mem.aux_anim =
            approach_half_life(mem.aux_anim, target_focus, dt, style.motion.state_half_life);
        let hover_anim = mem.hover_anim;
        let focus_anim = mem.aux_anim;

        // ── Background + border (focus = 1px focus_ring swap, no glow).
        let fill = self.fill.unwrap_or(style.palette.input);
        let stroke = lerp_color(
            lerp_color(style.palette.stroke, style.palette.stroke_strong, hover_anim),
            style.palette.focus_ring,
            focus_anim,
        );
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, style.rounding.widget, fill)
                .with_stroke(style.metrics.border, stroke),
        ));

        let text_y = rect.min.y + (rect.height() - font) * 0.5;

        // ── Content wider than the field: scroll under the caret while the
        //    user is typing, elide at rest. Either way the glyphs are clipped
        //    to the field, so a long value can never paint over whatever the
        //    field sits on (a node body, a table cell, the row beside it).
        let inner_w = (width - text_pad * 2.0).max(0.0);
        let full_w = measure_prefix(ui.text_mut(), self.value, font, self.value.len());
        let scroll = if focused {
            (caret_off - inner_w).max(0.0)
        } else {
            0.0
        };
        if let Some(s) = ui.ctx_mut().memory.data_get_mut::<TextEditState>(id) {
            s.scroll = scroll;
        }
        let text_x = text_x - scroll;
        let text_pos = Pos2::new(text_x, text_y);
        ui.ctx_mut().paint.push(PaintCmd::PushClip(Rect::from_min_max(
            Pos2::new(rect.min.x + 1.0, rect.min.y),
            Pos2::new(rect.max.x - 1.0, rect.max.y),
        )));

        // Selection highlight, drawn under the glyphs.
        if focused && sel_a != sel_b {
            let mut hl = style.palette.accent_active;
            hl.a = 0.30;
            let sel_rect = Rect::from_min_max(
                Pos2::new(text_x + sel_a_off, text_y - 1.0),
                Pos2::new(text_x + sel_b_off, text_y + font + 1.0),
            );
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                sel_rect,
                style.rounding.small,
                hl,
            )));
        }

        // Text (or dimmed hint when empty + unfocused). The hint fades well
        // below the dim text color (egui's "weak" text) so it can't be
        // mistaken for a value.
        let (display_text, display_color) = if self.value.is_empty() && !focused {
            let mut c = style.palette.text_secondary;
            c.a *= 0.55;
            (self.hint.as_deref().unwrap_or("").to_string(), c)
        } else if !focused && full_w > inner_w {
            // At rest an over-long value ends in an ellipsis: a hard clip mid
            // glyph reads as a rendering fault, "…" reads as "there is more".
            (elide(ui.text_mut(), self.value, font, inner_w), style.palette.text)
        } else {
            (self.value.clone(), style.palette.text)
        };
        if !display_text.is_empty() {
            let Ui { ctx, text, .. } = ui;
            text.add_text(&mut ctx.paint, &display_text, font, text_pos, display_color, None);
        }

        // Caret. Blinks while focused.
        if focused {
            let phase = (ui.ctx().time() * 2.0).fract();
            let blink_on = phase < 0.55;
            // Wake the host exactly at the next on/off toggle (phase advances
            // 2/sec); without this a static frame would freeze the blink.
            let to_boundary = if blink_on { 0.55 - phase } else { 1.0 - phase };
            ui.ctx_mut().request_repaint_after(std::time::Duration::from_secs_f64(
                to_boundary / 2.0 + 0.005,
            ));
            if blink_on {
                let caret_x = text_x + caret_off + 1.0;
                let caret_rect = Rect::from_min_size(
                    Pos2::new(caret_x, text_y - 1.0),
                    Vec2::new(1.5, font + 2.0),
                );
                ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                    caret_rect,
                    style.rounding.small,
                    style.palette.accent_active,
                )));
            }
        }
        ui.ctx_mut().paint.push(PaintCmd::PopClip);

        TextEditOutput {
            response: Response {
                id,
                rect,
                hovered,
                pressed: focused,
                clicked,
                hover_anim,
                press_anim: focus_anim,
            },
            submitted,
            cancelled,
            nav_up,
            nav_down,
            // Re-read after Enter/Escape handling so a focus drop this
            // frame is reflected.
            focused: ui.ctx().memory.focused_widget == Some(id),
        }
    }
}

// ─── editing helpers (byte-offset based) ─────────────────────────────────────

fn sel_range(st: &TextEditState) -> (usize, usize) {
    (st.cursor.min(st.anchor), st.cursor.max(st.anchor))
}

fn selected(value: &str, st: &TextEditState) -> Option<String> {
    let (s, e) = sel_range(st);
    (s != e).then(|| value[s..e].to_string())
}

fn replace_selection(value: &mut String, st: &mut TextEditState, ins: &str) {
    let (s, e) = sel_range(st);
    value.replace_range(s..e, ins);
    st.cursor = s + ins.len();
    st.anchor = st.cursor;
}

fn delete_selection(value: &mut String, st: &mut TextEditState) {
    let (s, e) = sel_range(st);
    value.replace_range(s..e, "");
    st.cursor = s;
    st.anchor = s;
}

fn backspace(value: &mut String, st: &mut TextEditState) {
    let (s, e) = sel_range(st);
    if s != e {
        delete_selection(value, st);
    } else if st.cursor > 0 {
        let p = prev_boundary(value, st.cursor);
        value.replace_range(p..st.cursor, "");
        st.cursor = p;
        st.anchor = p;
    }
}

fn delete_forward(value: &mut String, st: &mut TextEditState) {
    let (s, e) = sel_range(st);
    if s != e {
        delete_selection(value, st);
    } else if st.cursor < value.len() {
        let n = next_boundary(value, st.cursor);
        value.replace_range(st.cursor..n, "");
    }
}

fn move_left(value: &str, st: &mut TextEditState, shift: bool) {
    let (s, e) = sel_range(st);
    if !shift && s != e {
        st.cursor = s;
        st.anchor = s;
        return;
    }
    let p = prev_boundary(value, st.cursor);
    st.cursor = p;
    if !shift {
        st.anchor = p;
    }
}

fn move_right(value: &str, st: &mut TextEditState, shift: bool) {
    let (s, e) = sel_range(st);
    if !shift && s != e {
        st.cursor = e;
        st.anchor = e;
        return;
    }
    let n = next_boundary(value, st.cursor);
    st.cursor = n;
    if !shift {
        st.anchor = n;
    }
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].chars().next_back().map_or(0, |c| i - c.len_utf8())
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

fn clamp_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut j = i;
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

/// Pixel width of `value[..idx]` at `font` size (the x offset of a caret
/// sitting before byte `idx`).
/// Longest head of `value` that fits `max_w` once an ellipsis is appended.
/// Tail-trimmed rather than middle-truncated: a field reads left to right, and
/// the start of a value is what identifies it.
fn elide(text: &mut TextRenderer, value: &str, font: f32, max_w: f32) -> String {
    const ELLIPSIS: char = '\u{2026}';
    if max_w <= 0.0 {
        return String::new();
    }
    let mut keep = value.len();
    while keep > 0 {
        keep -= 1;
        while keep > 0 && !value.is_char_boundary(keep) {
            keep -= 1;
        }
        let mut candidate = value[..keep].to_string();
        candidate.push(ELLIPSIS);
        if text.measure(&candidate, font, None).x <= max_w {
            return candidate;
        }
    }
    ELLIPSIS.to_string()
}

fn measure_prefix(text: &mut TextRenderer, value: &str, font: f32, idx: usize) -> f32 {
    if idx == 0 {
        0.0
    } else {
        text.measure(&value[..idx], font, None).x
    }
}

/// Byte index of the character boundary whose caret x is nearest `target_x`.
fn byte_index_at_x(
    text: &mut TextRenderer,
    value: &str,
    font: f32,
    origin_x: f32,
    target_x: f32,
) -> usize {
    let local = (target_x - origin_x).max(0.0);
    let mut best = 0;
    let mut best_dx = f32::MAX;
    let mut idx = 0;
    loop {
        let w = measure_prefix(text, value, font, idx);
        let dx = (w - local).abs();
        if dx < best_dx {
            best_dx = dx;
            best = idx;
        }
        if idx >= value.len() {
            break;
        }
        idx = next_boundary(value, idx);
    }
    best
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        lerp(a.r, b.r, t),
        lerp(a.g, b.g, t),
        lerp(a.b, b.b, t),
        lerp(a.a, b.a, t),
    )
}
