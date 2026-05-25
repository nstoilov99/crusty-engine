//! Themed styling tokens read by every widget.
//!
//! Phase 5 introduces a single [`Style`] struct that bundles the colour
//! palette, spacing, default widget sizes, font sizes, rounding values
//! and animation timing the rest of the library uses to draw itself.
//! Widgets no longer hard-code these — they pull from
//! [`Ui::style`](crate::context::Ui::style).
//!
//! Two ready-made themes ship:
//! * [`Style::editor_dark`] — dense slate-gray palette modelled on
//!   game-engine property panels (Unreal/Unity-style). New default.
//! * [`Style::glassmorphism`] — translucent navy-blue panels with bigger
//!   defaults and vivid accents, matching the look from Phase 1.5.

use crate::math::{Color, Rounding, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub palette: Palette,
    pub spacing: Spacing,
    pub sizes: Sizes,
    pub fonts: Fonts,
    pub rounding: RoundingSet,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Quiet surface used for widget backgrounds (button fill, slider
    /// track, checkbox box).
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_active: Color,
    /// Vivid colour used for selection states (checkbox check, slider
    /// fill, radio dot, focus outline).
    pub accent: Color,
    /// Same accent at low alpha — used for hover glow / soft highlights.
    pub accent_glow: Color,
    pub text: Color,
    pub text_dim: Color,
    pub stroke: Color,
    pub stroke_hover: Color,
    /// Color for progress-bar / status fills (separate from accent so a
    /// theme can use orange accent + green progress, etc.).
    pub success: Color,
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
}

#[derive(Clone, Copy, Debug)]
pub struct RoundingSet {
    pub panel: Rounding,
    pub widget: Rounding,
    pub small: Rounding,
}

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub hover_half_life: f32,
    pub press_half_life: f32,
    pub state_half_life: f32,
}

impl Style {
    /// Dense slate-gray palette aimed at game-engine property panels.
    /// Smaller defaults than the glassmorphism theme, sharper corners,
    /// warm orange accent reminiscent of Unreal's selection highlight.
    pub fn editor_dark() -> Self {
        Self {
            palette: Palette {
                surface: Color::rgba(0.16, 0.17, 0.20, 0.92),
                surface_hover: Color::rgba(0.22, 0.24, 0.28, 0.95),
                surface_active: Color::rgba(0.12, 0.13, 0.16, 0.95),
                accent: Color::rgba(0.96, 0.56, 0.18, 0.95),
                accent_glow: Color::rgba(0.96, 0.56, 0.18, 0.25),
                text: Color::rgba(0.92, 0.93, 0.95, 0.96),
                text_dim: Color::rgba(0.62, 0.66, 0.72, 0.85),
                stroke: Color::rgba(0.05, 0.06, 0.08, 0.85),
                stroke_hover: Color::rgba(0.96, 0.56, 0.18, 0.75),
                success: Color::rgba(0.42, 0.78, 0.46, 0.95),
            },
            spacing: Spacing {
                item: 4.0,
                padding: 8.0,
                button_padding: Vec2::new(10.0, 5.0),
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
                body: 13.0,
                title: 18.0,
                small: 11.0,
            },
            rounding: RoundingSet {
                panel: Rounding::same(4.0),
                widget: Rounding::same(3.0),
                small: Rounding::same(2.0),
            },
            motion: Motion {
                // Editor rows / buttons want near-instant hover/press
                // feedback (Unity/Unreal-style). Smooth state transitions
                // (check, collapse) stay slower for polish.
                hover_half_life: 0.012,
                press_half_life: 0.012,
                state_half_life: 0.06,
            },
        }
    }

    /// Translucent navy panels with vivid blue accents and generous
    /// spacing. The look the demo has been showing since Phase 1.5.
    pub fn glassmorphism() -> Self {
        Self {
            palette: Palette {
                surface: Color::rgba(0.10, 0.13, 0.18, 0.75),
                surface_hover: Color::rgba(0.18, 0.30, 0.55, 0.80),
                surface_active: Color::rgba(0.12, 0.22, 0.45, 0.85),
                accent: Color::rgba(0.30, 0.55, 0.95, 0.90),
                accent_glow: Color::rgba(0.45, 0.65, 1.0, 0.25),
                text: Color::rgba(0.96, 0.97, 1.00, 0.95),
                text_dim: Color::rgba(0.78, 0.86, 1.00, 0.80),
                stroke: Color::rgba(1.00, 1.00, 1.00, 0.22),
                stroke_hover: Color::rgba(1.00, 1.00, 1.00, 0.55),
                success: Color::rgba(0.30, 0.75, 0.55, 0.95),
            },
            spacing: Spacing {
                item: 8.0,
                padding: 12.0,
                button_padding: Vec2::new(16.0, 10.0),
                box_label_gap: 8.0,
            },
            sizes: Sizes {
                checkbox: 18.0,
                radio: 18.0,
                slider_height: 22.0,
                slider_default_width: 220.0,
                slider_track_ratio: 0.32,
                progress_height: 14.0,
                progress_default_width: 220.0,
            },
            fonts: Fonts {
                body: 16.0,
                title: 40.0,
                small: 13.0,
            },
            rounding: RoundingSet {
                panel: Rounding::same(20.0),
                widget: Rounding::same(8.0),
                small: Rounding::same(4.0),
            },
            motion: Motion {
                hover_half_life: 0.025,
                press_half_life: 0.020,
                state_half_life: 0.08,
            },
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::editor_dark()
    }
}
