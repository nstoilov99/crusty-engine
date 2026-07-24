//! Themed styling tokens read by every widget.
//!
//! M10 rebuilds [`Style`] around the Crusty Design System's semantic
//! tokens: an opaque surface stack (depth = lighter fill + 1px border,
//! never shadows), job-split accents, a preset-invariant selection pair,
//! and metric tokens. Widgets never hard-code a color that exists here —
//! "if you type a hex value in widget code, it's a bug."
//!
//! [`Style::steel`] is the built-in default (Steel preset). The engine
//! feeds full preset themes through this struct at runtime; crusty-gui
//! itself ships only Steel.

use crate::math::{Color, Rounding, Vec2};

/// sRGB 8-bit shorthand for palette literals.
#[inline]
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_srgb_u8(r, g, b, 255)
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub palette: Palette,
    pub spacing: Spacing,
    pub sizes: Sizes,
    pub fonts: Fonts,
    pub rounding: RoundingSet,
    pub metrics: Metrics,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    // --- Opaque surface stack, darkest → lightest -------------------------
    /// Editable fields (TextEdit, DragValue, spinbox) — darkest, reads inset.
    pub input: Color,
    /// Main window background, tab strips, status bar.
    pub window: Color,
    /// Panel bodies.
    pub panel: Color,
    /// Section headers, spinbox caps, quiet widget fills.
    pub header: Color,
    /// Popovers, dialogs, toasts, dropdown lists (E3/E4).
    pub elevated: Color,
    /// Pointer-over fill for rows, menu items, ghost buttons.
    pub hover: Color,
    /// Pressed/active neutral fill.
    pub active: Color,
    /// 1px default border.
    pub stroke: Color,
    /// 1px border for hovered controls + popover/dialog outlines.
    pub stroke_strong: Color,
    // --- Accents — one job per token --------------------------------------
    /// Active tab underline, toggled tools, checked states, slider/progress
    /// fill.
    pub accent_active: Color,
    /// Focused inputs & keyboard nav: 1px border swap, no glow.
    pub focus_ring: Color,
    /// `accent_active` at ~15% alpha: fill behind toggled tool buttons,
    /// active filter chips.
    pub accent_soft: Color,
    // --- Preset-invariant selection ---------------------------------------
    /// Hierarchy rows, asset tiles, dropdown current value.
    pub selection_fill: Color,
    /// Text/icons on top of `selection_fill`.
    pub selection_text: Color,
    // --- Text --------------------------------------------------------------
    pub text: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    /// Numeric values, paths, logs (mono runs).
    pub text_mono: Color,
    // --- Transient-surface alphas ------------------------------------------
    /// Alpha for transient surfaces (menus, dropdowns, tooltips) only.
    /// 1.0 disables translucency. Simple alpha — no blur, no shadows.
    pub popover_alpha: f32,
    /// Scrim behind modals.
    pub scrim_alpha: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Spacing {
    /// Vertical space between widgets in a column layout.
    pub item: f32,
    /// Default padding inside containers (panel, window).
    pub padding: f32,
    /// Padding inside a button between the label text and the border.
    pub button_padding: Vec2,
    /// Gap between a checkbox/radio's box and its label.
    pub box_label_gap: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub checkbox: f32,
    pub radio: f32,
    pub slider_height: f32,
    pub slider_default_width: f32,
    pub slider_track_ratio: f32,
    pub progress_height: f32,
    pub progress_default_width: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Fonts {
    pub body: f32,
    pub title: f32,
    pub small: f32,
    /// Size for the bundled mono family (values, paths, shortcuts, logs).
    pub mono: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct RoundingSet {
    pub panel: Rounding,
    pub widget: Rounding,
    pub small: Rounding,
}

/// Non-color metric tokens (Comfortable density baseline).
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// The only border width.
    pub border: f32,
    /// Reserved: active-tab top edge + asset-tile type edge.
    pub edge_accent: f32,
    /// List/tree/menu row height.
    pub row_height: f32,
    /// Buttons, inputs, dropdowns.
    pub control_height: f32,
    /// Tree indent per level.
    pub indent: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub hover_half_life: f32,
    pub press_half_life: f32,
    pub state_half_life: f32,
}

impl Style {
    /// Steel — the design system's default preset. Cool neutral surfaces,
    /// steel-blue accent.
    pub fn steel() -> Self {
        Self {
            palette: Palette {
                input: rgb(14, 14, 17),
                window: rgb(18, 18, 21),
                panel: rgb(24, 24, 28),
                header: rgb(30, 31, 35),
                elevated: rgb(34, 35, 39),
                hover: rgb(48, 49, 54),
                active: rgb(60, 62, 68),
                stroke: rgb(55, 57, 63),
                stroke_strong: rgb(74, 77, 85),
                accent_active: rgb(79, 163, 232),
                focus_ring: rgb(122, 187, 240),
                accent_soft: rgb(79, 163, 232).with_alpha(0.15),
                selection_fill: rgb(60, 70, 84),
                selection_text: Color::WHITE,
                text: rgb(232, 234, 237),
                text_secondary: rgb(170, 176, 182),
                text_disabled: rgb(116, 122, 128),
                text_mono: rgb(198, 203, 211),
                popover_alpha: 0.96,
                scrim_alpha: 0.45,
            },
            spacing: Spacing {
                item: 4.0,
                padding: 8.0,
                button_padding: Vec2::new(12.0, 4.0),
                box_label_gap: 6.0,
            },
            sizes: Sizes {
                checkbox: 14.0,
                radio: 14.0,
                slider_height: 16.0,
                slider_default_width: 180.0,
                slider_track_ratio: 0.35,
                progress_height: 8.0,
                progress_default_width: 180.0,
            },
            fonts: Fonts {
                body: 12.0,
                title: 18.0,
                small: 10.5,
                mono: 12.0,
            },
            rounding: RoundingSet {
                panel: Rounding::same(6.0),
                widget: Rounding::same(3.0),
                small: Rounding::same(2.0),
            },
            metrics: Metrics {
                border: 1.0,
                edge_accent: 2.0,
                row_height: 22.0,
                control_height: 24.0,
                indent: 18.0,
            },
            motion: Motion {
                // Editor rows / buttons want near-instant hover/press
                // feedback. Smooth state transitions (check, collapse)
                // stay slower for polish.
                hover_half_life: 0.012,
                press_half_life: 0.012,
                state_half_life: 0.06,
            },
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::steel()
    }
}
