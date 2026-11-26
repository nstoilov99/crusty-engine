use super::{Pos2, Vec2};

/// Axis-aligned rectangle in screen-space (logical pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rect {
    pub min: Pos2,
    pub max: Pos2,
}

impl Rect {
    pub const ZERO: Self = Self {
        min: Pos2::ZERO,
        max: Pos2::ZERO,
    };

    #[inline]
    pub const fn from_min_max(min: Pos2, max: Pos2) -> Self {
        Self { min, max }
    }

    #[inline]
    pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    #[inline]
    pub fn from_center_size(center: Pos2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    #[inline]
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    #[inline]
    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    #[inline]
    pub fn center(self) -> Pos2 {
        Pos2::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    #[inline]
    pub fn contains(self, p: Pos2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            min: Pos2::new(self.min.x.max(other.min.x), self.min.y.max(other.min.y)),
            max: Pos2::new(self.max.x.min(other.max.x), self.max.y.min(other.max.y)),
        }
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Pos2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Pos2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    #[inline]
    pub fn shrink(self, by: f32) -> Self {
        self.shrink2(Vec2::splat(by))
    }

    #[inline]
    pub fn shrink2(self, by: Vec2) -> Self {
        Self {
            min: self.min + by,
            max: self.max - by,
        }
    }

    #[inline]
    pub fn expand(self, by: f32) -> Self {
        self.shrink(-by)
    }

    #[inline]
    pub fn translate(self, by: Vec2) -> Self {
        Self {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// True when min ≤ max on both axes.
    #[inline]
    pub fn is_valid(self) -> bool {
        self.max.x >= self.min.x && self.max.y >= self.min.y
    }
}
