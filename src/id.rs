//! Stable widget identity.
//!
//! An [`Id`] is a hashed `u64` that survives between frames. Widgets are
//! identified by hashing a user-supplied source (a label, a number, a type)
//! against their parent's `Id` — this gives every widget a unique, stable key
//! without requiring the caller to track positions or indices.

use ahash::AHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(pub u64);

impl Id {
    pub const NULL: Self = Id(0);

    /// Root id — used at the top of the tree where there is no parent.
    pub const ROOT: Self = Id(0xC0DE_C0DE_C0DE_C0DE);

    /// Build an Id by hashing `source` against `self` (the parent).
    pub fn with<S: Hash>(self, source: S) -> Self {
        let mut hasher = AHasher::default();
        self.0.hash(&mut hasher);
        source.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Build a top-level Id from a source.
    pub fn new<S: Hash>(source: S) -> Self {
        Self::ROOT.with(source)
    }
}
