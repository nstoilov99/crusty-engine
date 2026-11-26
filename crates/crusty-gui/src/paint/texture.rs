//! Backend-agnostic texture identity.
//!
//! A [`TextureId`] is an opaque handle the renderer hands back when a user
//! registers a GPU image (see the backend's `TextureRegistry`). Widgets and
//! paint commands only ever carry the id — the actual `ImageView` lives in
//! the backend, keeping the paint layer free of any graphics-API types.

/// Opaque handle to a renderer-side texture. Cheap to copy and store.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TextureId(pub u64);

/// Sampling filter requested when registering a texture.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum TextureFilter {
    /// Bilinear — smooth scaling, good for photos and thumbnails.
    #[default]
    Linear,
    /// Nearest — crisp pixels, good for icon atlases / pixel art.
    Nearest,
}
