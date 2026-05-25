/// Per-corner border radii. All values in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rounding {
    pub nw: f32,
    pub ne: f32,
    pub sw: f32,
    pub se: f32,
}

impl Rounding {
    pub const ZERO: Self = Self::same(0.0);

    #[inline]
    pub const fn same(r: f32) -> Self {
        Self {
            nw: r,
            ne: r,
            sw: r,
            se: r,
        }
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.nw == 0.0 && self.ne == 0.0 && self.sw == 0.0 && self.se == 0.0
    }

    #[inline]
    pub fn max_radius(self) -> f32 {
        self.nw.max(self.ne).max(self.sw).max(self.se)
    }
}

impl From<f32> for Rounding {
    fn from(r: f32) -> Self {
        Self::same(r)
    }
}
