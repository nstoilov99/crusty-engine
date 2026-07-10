use crate::context::{Direction, Ui, UiOptions};
use crate::input::Key;
use crate::layer::Order;
use crate::math::{Color, Pos2, Rect, Rounding, Vec2};
use crate::paint::{PaintCmd, RectShape};
use crate::response::Response;
use crate::widgets::{DragValue, TextEdit};

/// HSV working state. Persisted so hue/saturation survive passes through
/// degenerate RGB values (grays lose hue, black loses saturation).
#[derive(Clone, Copy)]
struct PickerHsv {
    h: f32,
    s: f32,
    v: f32,
    /// sRGB snapshot of the color these HSV values were derived from;
    /// when the app changes the color externally we re-derive.
    srgb: [u8; 3],
}

/// Compact color field that opens a full picker popup: SV square, hue bar,
/// optional alpha bar, RGBA + hex entry, and a caller-owned row of cached
/// swatches (Unreal-style saved colors).
///
/// Deliberately *not* an egui clone — sized like a normal inspector field,
/// popup floats on the overlay layer like [`ComboBox`](crate::widgets::ComboBox).
pub struct ColorPicker<'a> {
    color: &'a mut Color,
    id_source: String,
    alpha: bool,
    width: Option<f32>,
    height: Option<f32>,
    swatches: Option<&'a mut Vec<Color>>,
}

const SV_W: f32 = 216.0;
const SV_H: f32 = 140.0;
const BAR_H: f32 = 12.0;
const CHIP: f32 = 16.0;
const CHIP_GAP: f32 = 4.0;
const MAX_SWATCHES: usize = 22;

impl<'a> ColorPicker<'a> {
    pub fn new(id_source: impl Into<String>, color: &'a mut Color) -> Self {
        Self {
            color,
            id_source: id_source.into(),
            alpha: true,
            width: None,
            height: None,
            swatches: None,
        }
    }

    /// Show the alpha bar + A field (default true).
    pub fn alpha(mut self, alpha: bool) -> Self {
        self.alpha = alpha;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    /// Caller-owned cached-colors store shown as a clickable swatch row
    /// (click applies, right-click removes, `+` chip saves the current
    /// color). Owned by the app so it can outlive the widget and be
    /// serialized with editor preferences.
    pub fn swatches(mut self, swatches: &'a mut Vec<Color>) -> Self {
        self.swatches = Some(swatches);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response {
        let style = ui.style();
        let id = ui.alloc_id(("color_picker", &self.id_source));
        let font = style.fonts.body;
        let width = self.width.unwrap_or(48.0);
        let height = self.height.unwrap_or(font + 6.0);
        let trigger_rect = ui.allocate(Vec2::new(width, height));

        let hovered = ui.contains_pointer(trigger_rect);
        let pointer_pressed = ui.ctx().input.pointer_pressed;
        let pointer_pos = ui.ctx().input.pointer_pos;
        if hovered {
            ui.ctx_mut().set_cursor_icon(crate::context::CursorIcon::Pointer);
        }

        let was_open = ui.ctx().memory.get(id).map(|m| m.bool_state).unwrap_or(false);
        let mut is_open = was_open;
        if pointer_pressed && hovered {
            is_open = !is_open;
        }

        // ── HSV working state, re-derived when the color changed externally.
        let cur = self.color.to_srgb_u8();
        let mut hsv = ui
            .ctx()
            .memory
            .data_get::<PickerHsv>(id)
            .copied()
            .unwrap_or_else(|| derive_hsv(cur, None));
        if hsv.srgb != [cur[0], cur[1], cur[2]] {
            hsv = derive_hsv(cur, Some(hsv));
        }

        // ── Trigger: checkerboard under a translucent color swatch.
        if self.alpha && self.color.a < 1.0 {
            push_checker(&mut ui.ctx_mut().paint, trigger_rect.shrink(1.0), 5.0);
        }
        let stroke = if is_open {
            style.palette.accent
        } else if hovered {
            style.palette.text_dim
        } else {
            style.palette.stroke
        };
        ui.ctx_mut().paint.push(PaintCmd::Rrect(
            RectShape::fill(trigger_rect, Rounding::same(3.0), *self.color)
                .with_stroke(1.0, stroke),
        ));

        // ── Popup.
        if is_open {
            let popup_w = SV_W + 16.0;
            let popup_x = trigger_rect
                .min
                .x
                .min(ui.ctx().screen_rect().max.x - popup_w - 4.0);
            let popup_y = trigger_rect.max.y + 2.0;
            let scratch_rect = Rect::from_min_size(
                Pos2::new(popup_x, popup_y),
                Vec2::new(popup_w, 600.0),
            );
            let body_start_main = ui.ctx().paint.len();
            let popup_area_id = id.with("popup_area");

            let used_h = {
                let Ui { ctx, text, .. } = ui;
                let opts = UiOptions {
                    padding: Vec2::new(8.0, 8.0),
                    spacing: 6.0,
                };
                let mut inner = Ui::with_root(
                    ctx,
                    text,
                    scratch_rect,
                    Direction::TopDown,
                    opts,
                    id.with("popup"),
                );
                inner.set_area(Some(popup_area_id));
                self.popup_body(&mut inner, id, &mut hsv);
                inner.used_rect().height()
            };

            let drained: Vec<PaintCmd> = ui.ctx_mut().paint.drain(body_start_main..).collect();
            let popup_rect = Rect::from_min_size(
                Pos2::new(popup_x, popup_y),
                Vec2::new(popup_w, used_h + 16.0),
            );
            ui.ctx_mut().overlay_paint.push(PaintCmd::Rrect(
                RectShape::fill(popup_rect, style.rounding.widget, style.palette.surface_hover)
                    .with_stroke(1.0, style.palette.stroke)
                    .with_shadow(20.0, Color::rgba(0.0, 0.0, 0.0, 0.45)),
            ));
            ui.ctx_mut().overlay_paint.extend(drained);

            let area = ui
                .ctx_mut()
                .memory
                .area_entry(popup_area_id, popup_rect.min, Order::Tooltip);
            area.pos = popup_rect.min;
            area.size = popup_rect.size();

            if ui.ctx().memory.popup_close_requested {
                is_open = false;
            }
            if ui.ctx().input.key_pressed(Key::Escape) {
                is_open = false;
            }
            if pointer_pressed
                && let Some(p) = pointer_pos
                && !trigger_rect.contains(p)
                && !popup_rect.contains(p)
            {
                is_open = false;
            }
        }

        ui.ctx_mut().memory.data_insert(id, hsv);
        let mem = ui.ctx_mut().memory.get_or_default(id);
        mem.seen_this_frame = true;
        mem.last_rect = trigger_rect;
        mem.bool_state = is_open;

        Response {
            id,
            rect: trigger_rect,
            hovered,
            pressed: false,
            clicked: pointer_pressed && hovered,
            hover_anim: if hovered { 1.0 } else { 0.0 },
            press_anim: 0.0,
        }
    }

    fn popup_body(&mut self, ui: &mut Ui, id: crate::id::Id, hsv: &mut PickerHsv) {
        let style = ui.style();
        let text_dim = style.palette.text_dim;

        // ── SV square.
        let sq = ui.allocate(Vec2::new(SV_W, SV_H));
        let sq_resp = ui.interact(id.with("sv"), sq);
        if sq_resp.pressed
            && let Some(p) = ui.ctx().input.pointer_pos
        {
            hsv.s = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
            hsv.v = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
            self.apply_hsv(hsv);
        }
        paint_sv_square(&mut ui.ctx_mut().paint, sq, hsv.h);
        let cursor = Pos2::new(
            sq.min.x + hsv.s * sq.width(),
            sq.min.y + (1.0 - hsv.v) * sq.height(),
        );
        push_ring(&mut ui.ctx_mut().paint, cursor, 5.0);

        // ── Hue bar.
        let hue = ui.allocate(Vec2::new(SV_W, BAR_H));
        let hue_resp = ui.interact(id.with("hue"), hue);
        if hue_resp.pressed
            && let Some(p) = ui.ctx().input.pointer_pos
        {
            hsv.h = ((p.x - hue.min.x) / hue.width()).clamp(0.0, 1.0) * 360.0;
            self.apply_hsv(hsv);
        }
        paint_hue_bar(&mut ui.ctx_mut().paint, hue);
        push_bar_cursor(&mut ui.ctx_mut().paint, hue, hsv.h / 360.0);

        // ── Alpha bar.
        if self.alpha {
            let bar = ui.allocate(Vec2::new(SV_W, BAR_H));
            let resp = ui.interact(id.with("alpha"), bar);
            if resp.pressed
                && let Some(p) = ui.ctx().input.pointer_pos
            {
                self.color.a = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
            }
            push_checker(&mut ui.ctx_mut().paint, bar, 6.0);
            let opaque = self.color.with_alpha(1.0);
            ui.ctx_mut().paint.push(PaintCmd::GradientQuad {
                rect: bar,
                colors: [
                    opaque.with_alpha(0.0),
                    opaque,
                    opaque,
                    opaque.with_alpha(0.0),
                ],
            });
            ui.ctx_mut().paint.push(PaintCmd::Rrect(RectShape {
                rect: bar,
                rounding: Rounding::same(2.0),
                fill: None,
                stroke: Some(crate::paint::Stroke { width: 1.0, color: style.palette.stroke }),
                shadow: None,
                glow: None,
            }));
            push_bar_cursor(&mut ui.ctx_mut().paint, bar, self.color.a);
        }

        // ── RGBA fields (0–255 sRGB, matching hex).
        let srgb = self.color.to_srgb_u8();
        let mut vals = [
            srgb[0] as f32,
            srgb[1] as f32,
            srgb[2] as f32,
            srgb[3] as f32,
        ];
        let labels = ["R", "G", "B", "A"];
        let n = if self.alpha { 4 } else { 3 };
        let mut rgba_changed = false;
        ui.horizontal(|ui| {
            for i in 0..n {
                ui.label(labels[i]);
                let before = vals[i];
                DragValue::new(&mut vals[i])
                    .range(0.0..=255.0)
                    .speed(1.0)
                    .decimals(0)
                    .width(34.0)
                    .show(ui);
                if vals[i] != before {
                    rgba_changed = true;
                }
            }
        });
        if rgba_changed {
            let mut c = Color::from_srgb_u8(
                vals[0].round() as u8,
                vals[1].round() as u8,
                vals[2].round() as u8,
                255,
            );
            c.a = if self.alpha { vals[3] / 255.0 } else { self.color.a };
            *self.color = c;
            *hsv = derive_hsv(self.color.to_srgb_u8(), Some(*hsv));
        }

        // ── Hex field.
        let hex_now = format_hex(self.color.to_srgb_u8(), self.alpha);
        ui.horizontal(|ui| {
            ui.label("Hex");
            let hex_id = id.with("hex");
            let mut text = ui
                .ctx_mut()
                .memory
                .data_remove::<String>(hex_id)
                .unwrap_or_else(|| hex_now.clone());
            let out = TextEdit::new(&mut text).width(96.0).show_full(ui);
            if out.submitted
                && let Some(c) = parse_hex(&text, self.alpha, self.color.a)
            {
                *self.color = c;
                *hsv = derive_hsv(self.color.to_srgb_u8(), Some(*hsv));
            }
            if !out.focused {
                text = format_hex(self.color.to_srgb_u8(), self.alpha);
            }
            ui.ctx_mut().memory.data_insert(hex_id, text);
        });

        // ── Cached swatches (Unreal-style saved colors).
        if let Some(swatches) = self.swatches.as_deref_mut() {
            ui.separator();
            let per_row = ((SV_W + CHIP_GAP) / (CHIP + CHIP_GAP)).floor() as usize;
            let count = swatches.len() + 1; // plus the save chip
            let rows = count.div_ceil(per_row);
            let block = ui.allocate(Vec2::new(
                SV_W,
                rows as f32 * (CHIP + CHIP_GAP) - CHIP_GAP,
            ));

            let chip_rect = |i: usize| {
                let (row, col) = (i / per_row, i % per_row);
                Rect::from_min_size(
                    Pos2::new(
                        block.min.x + col as f32 * (CHIP + CHIP_GAP),
                        block.min.y + row as f32 * (CHIP + CHIP_GAP),
                    ),
                    Vec2::splat(CHIP),
                )
            };

            // Save chip: "+" on a plain field.
            let save = chip_rect(0);
            let save_resp = ui.interact(id.with("swatch_save"), save);
            let fill = if save_resp.hovered {
                style.palette.surface_hover
            } else {
                style.palette.surface_active
            };
            ui.ctx_mut().paint.push(PaintCmd::Rrect(
                RectShape::fill(save, Rounding::same(3.0), fill)
                    .with_stroke(1.0, style.palette.stroke),
            ));
            let c = save.center();
            let half = 4.0;
            ui.ctx_mut().paint.push(PaintCmd::Line {
                from: Pos2::new(c.x - half, c.y),
                to: Pos2::new(c.x + half, c.y),
                width: 1.0,
                color: text_dim,
            });
            ui.ctx_mut().paint.push(PaintCmd::Line {
                from: Pos2::new(c.x, c.y - half),
                to: Pos2::new(c.x, c.y + half),
                width: 1.0,
                color: text_dim,
            });
            if save_resp.clicked {
                let cur = *self.color;
                swatches.retain(|s| s.to_srgb_u8() != cur.to_srgb_u8());
                swatches.insert(0, cur);
                swatches.truncate(MAX_SWATCHES);
            }

            let right_pressed = ui.ctx().input.right_pressed;
            let mut remove: Option<usize> = None;
            let mut apply: Option<Color> = None;
            for (i, swatch) in swatches.iter().enumerate() {
                let rect = chip_rect(i + 1);
                let resp = ui.interact(id.with(("swatch", i)), rect);
                if swatch.a < 1.0 {
                    push_checker(&mut ui.ctx_mut().paint, rect.shrink(1.0), 4.0);
                }
                let stroke = if resp.hovered {
                    style.palette.accent
                } else {
                    style.palette.stroke
                };
                ui.ctx_mut().paint.push(PaintCmd::Rrect(
                    RectShape::fill(rect, Rounding::same(3.0), *swatch)
                        .with_stroke(1.0, stroke),
                ));
                if resp.clicked {
                    apply = Some(*swatch);
                }
                if right_pressed && resp.hovered {
                    remove = Some(i);
                }
            }
            if let Some(c) = apply {
                *self.color = if self.alpha { c } else { c.with_alpha(self.color.a) };
                *hsv = derive_hsv(self.color.to_srgb_u8(), Some(*hsv));
            }
            if let Some(i) = remove {
                swatches.remove(i);
            }
        }
    }

    fn apply_hsv(&mut self, hsv: &mut PickerHsv) {
        let a = self.color.a;
        *self.color = hsv_to_color(hsv.h, hsv.s, hsv.v).with_alpha(a);
        let s = self.color.to_srgb_u8();
        hsv.srgb = [s[0], s[1], s[2]];
    }
}

/// Derive HSV from 8-bit sRGB, preserving hue (and saturation at v=0) from
/// `prev` when the RGB value is degenerate.
fn derive_hsv(srgb: [u8; 4], prev: Option<PickerHsv>) -> PickerHsv {
    let r = srgb[0] as f32 / 255.0;
    let g = srgb[1] as f32 / 255.0;
    let b = srgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let mut s = if max > 0.0 { d / max } else { 0.0 };
    let mut h = if d <= f32::EPSILON {
        prev.map(|p| p.h).unwrap_or(0.0)
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if v <= f32::EPSILON
        && let Some(p) = prev
    {
        s = p.s;
        h = p.h;
    }
    PickerHsv { h, s, v, srgb: [srgb[0], srgb[1], srgb[2]] }
}

fn hsv_to_color(h: f32, s: f32, v: f32) -> Color {
    let h = h.rem_euclid(360.0) / 60.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color::from_srgb_u8(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        255,
    )
}

fn format_hex(srgb: [u8; 4], alpha: bool) -> String {
    if alpha {
        format!("#{:02X}{:02X}{:02X}{:02X}", srgb[0], srgb[1], srgb[2], srgb[3])
    } else {
        format!("#{:02X}{:02X}{:02X}", srgb[0], srgb[1], srgb[2])
    }
}

fn parse_hex(text: &str, alpha: bool, cur_a: f32) -> Option<Color> {
    let t = text.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok();
    match t.len() {
        6 => {
            let mut c = Color::from_srgb_u8(byte(0)?, byte(1)?, byte(2)?, 255);
            c.a = cur_a;
            Some(c)
        }
        8 if alpha => Some(Color::from_srgb_u8(byte(0)?, byte(1)?, byte(2)?, byte(3)?)),
        _ => None,
    }
}

/// SV gradient as an 8×8 grid of [`PaintCmd::GradientQuad`]s — interpolation
/// is per-triangle, so subdividing hides the diagonal seam.
fn paint_sv_square(paint: &mut Vec<PaintCmd>, rect: Rect, h: f32) {
    const SUB: usize = 8;
    let cw = rect.width() / SUB as f32;
    let ch = rect.height() / SUB as f32;
    for j in 0..SUB {
        for i in 0..SUB {
            let s0 = i as f32 / SUB as f32;
            let s1 = (i + 1) as f32 / SUB as f32;
            let v0 = 1.0 - j as f32 / SUB as f32;
            let v1 = 1.0 - (j + 1) as f32 / SUB as f32;
            let cell = Rect::from_min_size(
                Pos2::new(rect.min.x + i as f32 * cw, rect.min.y + j as f32 * ch),
                Vec2::new(cw, ch),
            );
            paint.push(PaintCmd::GradientQuad {
                rect: cell,
                colors: [
                    hsv_to_color(h, s0, v0),
                    hsv_to_color(h, s1, v0),
                    hsv_to_color(h, s1, v1),
                    hsv_to_color(h, s0, v1),
                ],
            });
        }
    }
}

/// Hue spectrum as 6 exact gradient segments (red→yellow→green→cyan→blue→magenta→red).
fn paint_hue_bar(paint: &mut Vec<PaintCmd>, rect: Rect) {
    let seg_w = rect.width() / 6.0;
    for i in 0..6 {
        let h0 = i as f32 * 60.0;
        let h1 = h0 + 60.0;
        let c0 = hsv_to_color(h0, 1.0, 1.0);
        let c1 = hsv_to_color(h1, 1.0, 1.0);
        let seg = Rect::from_min_size(
            Pos2::new(rect.min.x + i as f32 * seg_w, rect.min.y),
            Vec2::new(seg_w, rect.height()),
        );
        paint.push(PaintCmd::GradientQuad {
            rect: seg,
            colors: [c0, c1, c1, c0],
        });
    }
}

/// Small circular cursor: dark outer ring + white inner ring, readable on
/// any underlying color.
fn push_ring(paint: &mut Vec<PaintCmd>, center: Pos2, radius: f32) {
    for (r, w, color) in [
        (radius, 1.0, Color::rgba(0.0, 0.0, 0.0, 0.8)),
        (radius - 1.0, 1.5, Color::WHITE),
    ] {
        paint.push(PaintCmd::Rrect(RectShape {
            rect: Rect::from_center_size(center, Vec2::splat(r * 2.0)),
            rounding: Rounding::same(r),
            fill: None,
            stroke: Some(crate::paint::Stroke { width: w, color }),
            shadow: None,
            glow: None,
        }));
    }
}

/// Vertical pick line on a horizontal bar at normalized position `t`.
fn push_bar_cursor(paint: &mut Vec<PaintCmd>, bar: Rect, t: f32) {
    let x = bar.min.x + t.clamp(0.0, 1.0) * bar.width();
    let rect = Rect::from_center_size(
        Pos2::new(x, bar.center().y),
        Vec2::new(4.0, bar.height() + 4.0),
    );
    paint.push(PaintCmd::Rrect(RectShape {
        rect,
        rounding: Rounding::same(2.0),
        fill: Some(Color::WHITE),
        stroke: Some(crate::paint::Stroke {
            width: 1.0,
            color: Color::rgba(0.0, 0.0, 0.0, 0.8),
        }),
        shadow: None,
        glow: None,
    }));
}

/// Gray checkerboard signalling transparency, drawn as unrounded rects
/// (callers paint it under a rounded fill, inset so corners don't leak).
fn push_checker(paint: &mut Vec<PaintCmd>, rect: Rect, cell: f32) {
    let light = Color::from_srgb_u8(180, 180, 180, 255);
    let dark = Color::from_srgb_u8(110, 110, 110, 255);
    paint.push(PaintCmd::Rect { rect, color: light });
    let cols = (rect.width() / cell).ceil() as usize;
    let rows = (rect.height() / cell).ceil() as usize;
    for j in 0..rows {
        for i in 0..cols {
            if (i + j) % 2 == 0 {
                continue;
            }
            let min = Pos2::new(rect.min.x + i as f32 * cell, rect.min.y + j as f32 * cell);
            let max = Pos2::new(
                (min.x + cell).min(rect.max.x),
                (min.y + cell).min(rect.max.y),
            );
            paint.push(PaintCmd::Rect {
                rect: Rect { min, max },
                color: dark,
            });
        }
    }
}
