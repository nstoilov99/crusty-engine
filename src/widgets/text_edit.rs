use crate::animation::approach_half_life;
use crate::clipboard;
use crate::context::Ui;
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
}

/// Single-line text input.
///
/// Supports click-to-place and drag-to-select cursor, arrow / Home / End
/// navigation (Shift extends the selection), Backspace / Delete, select-all
/// (Ctrl+A), and system clipboard cut / copy / paste (Ctrl+X / C / V).
pub struct TextEdit<'a> {
    value: &'a mut String,
    width: Option<f32>,
    hint: Option<String>,
}

impl<'a> TextEdit<'a> {
    pub fn new(value: &'a mut String) -> Self {
        Self {
            value,
            width: None,
            hint: None,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Placeholder text shown (dimmed) when the value is empty.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id("text_edit");
        let font = style.fonts.body;
        let width = self
            .width
            .unwrap_or_else(|| ui.available().width().min(220.0));
        let height = font + 10.0;
        let rect = ui.allocate(Vec2::new(width, height));
        let text_pad = 6.0;
        let text_x = rect.min.x + text_pad;
        ui.ctx_mut().memory.register_focusable(id);

        // ── Snapshot input up front to keep borrows simple.
        let hovered = ui.contains_pointer(rect);
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
        let click_idx = if start_click || dragging {
            pointer_pos.map(|p| byte_index_at_x(ui.text_mut(), self.value, font, text_x, p.x))
        } else {
            None
        };

        if start_click {
            ui.ctx_mut().memory.focused_widget = Some(id);
            ui.ctx_mut().memory.active_widget = Some(id);
        }
        let focused = ui.ctx().memory.focused_widget == Some(id);
        let mut clicked = start_click;

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
                    Key::Escape | Key::Enter => {
                        ui.ctx_mut().memory.focused_widget = None;
                        clicked = true;
                    }
                    _ => {}
                }
            }
        }

        if pointer_released && active == Some(id) {
            ui.ctx_mut().memory.active_widget = None;
        }

        // Persist cursor/selection for next frame.
        ui.ctx_mut().memory.data_insert::<TextEditState>(id, st);

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

        // ── Background + border (accent stroke when focused).
        let fill = style.palette.surface_active;
        let stroke = lerp_color(
            lerp_color(style.palette.stroke, style.palette.stroke_hover, hover_anim),
            style.palette.accent,
            focus_anim,
        );
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(rect, style.rounding.widget, fill)
                .with_stroke(1.0 + focus_anim * 0.5, stroke),
        ));

        let text_y = rect.min.y + (rect.height() - font) * 0.5;
        let text_pos = Pos2::new(text_x, text_y);

        // Selection highlight, drawn under the glyphs.
        if focused && sel_a != sel_b {
            let mut hl = style.palette.accent;
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

        // Text (or dimmed hint when empty + unfocused).
        let (display_text, display_color) = if self.value.is_empty() && !focused {
            (self.hint.as_deref().unwrap_or(""), style.palette.text_dim)
        } else {
            (self.value.as_str(), style.palette.text)
        };
        if !display_text.is_empty() {
            let Ui { ctx, text, .. } = ui;
            text.add_text(&mut ctx.paint, display_text, font, text_pos, display_color, None);
        }

        // Caret. Blinks while focused.
        if focused {
            let blink_on = ((ui.ctx().time() * 2.0).fract()) < 0.55;
            if blink_on {
                let caret_x = text_x + caret_off + 1.0;
                let caret_rect = Rect::from_min_size(
                    Pos2::new(caret_x, text_y - 1.0),
                    Vec2::new(1.5, font + 2.0),
                );
                ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape::fill(
                    caret_rect,
                    style.rounding.small,
                    style.palette.accent,
                )));
            }
        }

        Response {
            id,
            rect,
            hovered,
            pressed: focused,
            clicked,
            hover_anim,
            press_anim: focus_anim,
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
