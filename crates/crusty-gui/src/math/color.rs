use bytemuck::{Pod, Zeroable};

/// Linear-space RGBA stored as 4 floats. The renderer assumes a linear-space
/// framebuffer; sRGB conversion happens at the boundary (e.g. when building
/// `Color::from_srgb_u8`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    #[inline]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    #[inline]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Convert from 8-bit sRGB to linear-space RGBA.
    #[inline]
    pub fn from_srgb_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: srgb_to_linear(r as f32 / 255.0),
            g: srgb_to_linear(g as f32 / 255.0),
            b: srgb_to_linear(b as f32 / 255.0),
            a: a as f32 / 255.0,
        }
    }

    #[inline]
    pub fn with_alpha(mut self, a: f32) -> Self {
        self.a = a;
        self
    }

    #[inline]
    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Convert back to 8-bit sRGB (inverse of [`Color::from_srgb_u8`]).
    #[inline]
    pub fn to_srgb_u8(self) -> [u8; 4] {
        [
            (linear_to_srgb(self.r) * 255.0).round().clamp(0.0, 255.0) as u8,
            (linear_to_srgb(self.g) * 255.0).round().clamp(0.0, 255.0) as u8,
            (linear_to_srgb(self.b) * 255.0).round().clamp(0.0, 255.0) as u8,
            (self.a * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }
}

#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
