//! Text shaping, glyph rasterization and atlas management.
//!
//! [`TextRenderer`] wraps cosmic-text's [`FontSystem`] + [`SwashCache`]
//! and a GPU [`GlyphAtlas`]. Call [`TextRenderer::add_text`] to append
//! `PaintCmd::Glyph` commands for a shaped string to a paint list — the
//! atlas is filled lazily as new glyphs appear.

mod atlas;

pub use atlas::{AtlasRect, GlyphAtlas};

use std::collections::HashMap;
use std::sync::Arc;

use ahash::AHasher;
use cosmic_text::{
    Attrs, Buffer, CacheKey, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
use vulkano::device::Device;
use vulkano::memory::allocator::StandardMemoryAllocator;

use crate::math::{Color, Pos2, Rect, Vec2};
use crate::paint::PaintCmd;

/// Cached per-glyph data: atlas position plus the swash placement we need
/// to position the quad on screen.
#[derive(Clone, Copy, Debug)]
struct CachedGlyph {
    atlas: AtlasRect,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    /// True when the glyph lives in the RGBA color page (emoji) rather than
    /// the R8 mask page.
    color: bool,
}

/// A fully shaped layout, kept across frames so repeated labels don't
/// re-run cosmic-text's (expensive) `Shaping::Advanced` every frame.
struct ShapedText {
    buffer: Buffer,
    size: Vec2,
    last_used: u64,
}

pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: GlyphAtlas,
    cache: HashMap<u64, Option<CachedGlyph>, std::hash::BuildHasherDefault<AHasher>>,
    shape_cache: HashMap<u64, ShapedText, std::hash::BuildHasherDefault<AHasher>>,
    frame: u64,
}

impl TextRenderer {
    pub fn new(device: Arc<Device>, atlas_extent: [u32; 2]) -> Self {
        let memory_allocator =
            Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let atlas = GlyphAtlas::new(memory_allocator, device, atlas_extent);
        Self {
            font_system,
            swash_cache,
            atlas,
            cache: HashMap::default(),
            shape_cache: HashMap::default(),
            frame: 0,
        }
    }

    /// Advance the frame counter and drop shaped layouts that haven't been
    /// touched recently. Call once per frame.
    pub fn new_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        self.shape_cache
            .retain(|_, s| frame.wrapping_sub(s.last_used) < 120);
    }

    /// Shape `text` (or reuse a cached layout) and return the cache key.
    fn ensure_shaped(&mut self, text: &str, size_px: f32, max_width: Option<f32>) -> u64 {
        let hash = shape_key_hash(text, size_px, max_width);
        if let Some(s) = self.shape_cache.get_mut(&hash) {
            s.last_used = self.frame;
            return hash;
        }

        let line_height = (size_px * 1.3).max(size_px + 2.0);
        let metrics = Metrics::new(size_px, line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let size = measure_buffer(&buffer, size_px);
        self.shape_cache.insert(
            hash,
            ShapedText {
                buffer,
                size,
                last_used: self.frame,
            },
        );
        hash
    }

    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    pub fn atlas_mut(&mut self) -> &mut GlyphAtlas {
        &mut self.atlas
    }

    /// Shape `text` and return its tight bounding-box size in physical
    /// pixels. Does not rasterise or upload glyphs — pair with a
    /// subsequent [`add_text`](Self::add_text) at the same parameters to
    /// emit paint commands.
    pub fn measure(&mut self, text: &str, size_px: f32, max_width: Option<f32>) -> Vec2 {
        let hash = self.ensure_shaped(text, size_px, max_width);
        self.shape_cache[&hash].size
    }

    /// Shape `text` at `size_px` and append a [`PaintCmd::Glyph`] for
    /// every glyph. `position` is the top-left corner of the layout box.
    /// `max_width` constrains line breaks (`None` = unbounded).
    pub fn add_text(
        &mut self,
        paint: &mut Vec<PaintCmd>,
        text: &str,
        size_px: f32,
        position: Pos2,
        color: Color,
        max_width: Option<f32>,
    ) {
        let hash = self.ensure_shaped(text, size_px, max_width);

        // Collect physical glyph placements first (borrows the shaped
        // buffer), then rasterise — `ensure_cached` needs `&mut self`.
        let mut pending: Vec<(CacheKey, i32, i32)> = Vec::new();
        {
            let shaped = &self.shape_cache[&hash];
            for run in shaped.buffer.layout_runs() {
                let baseline_y = position.y + run.line_y;
                for glyph in run.glyphs.iter() {
                    let physical = glyph.physical((position.x, baseline_y), 1.0);
                    pending.push((physical.cache_key, physical.x, physical.y));
                }
            }
        }

        for (key, px, py) in pending {
            let Some(cached) = self.ensure_cached(key) else {
                continue;
            };
            let quad_pos = Pos2::new(
                px as f32 + cached.left as f32,
                py as f32 - cached.top as f32,
            );
            let quad_size = Vec2::new(cached.width as f32, cached.height as f32);
            if quad_size.x <= 0.0 || quad_size.y <= 0.0 {
                continue;
            }
            let rect = Rect::from_min_size(quad_pos, quad_size);
            if cached.color {
                paint.push(PaintCmd::ColorGlyph {
                    rect,
                    uv_min: cached.atlas.uv_min,
                    uv_max: cached.atlas.uv_max,
                    alpha: color.a,
                });
            } else {
                paint.push(PaintCmd::Glyph {
                    rect,
                    uv_min: cached.atlas.uv_min,
                    uv_max: cached.atlas.uv_max,
                    color,
                });
            }
        }
    }

    fn ensure_cached(&mut self, key: CacheKey) -> Option<CachedGlyph> {
        let hash = cache_key_hash(&key);
        if let Some(entry) = self.cache.get(&hash) {
            return *entry;
        }

        let image = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, key);
        let Some(image) = image else {
            self.cache.insert(hash, None);
            return None;
        };

        // Zero-extent glyphs (e.g. space) still get a non-None None entry
        // so we don't redo the work, but we have nothing to draw.
        if image.placement.width == 0 || image.placement.height == 0 {
            self.cache.insert(hash, None);
            return None;
        }

        // Mask / subpixel glyphs go to the R8 page; color glyphs (emoji) to
        // the RGBA page (Phase 10).
        let is_color = matches!(image.content, SwashContent::Color);
        let atlas_rect = match image.content {
            SwashContent::Mask => {
                self.atlas
                    .insert(image.placement.width, image.placement.height, image.data.clone())
            }
            SwashContent::SubpixelMask => {
                // SubpixelMask stores 3 sub-channels per pixel; collapse to
                // luminance by averaging.
                let bytes: Vec<u8> = image
                    .data
                    .chunks_exact(3)
                    .map(|c| ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8)
                    .collect();
                self.atlas
                    .insert(image.placement.width, image.placement.height, bytes)
            }
            SwashContent::Color => {
                // swash hands us straight-alpha RGBA, 4 bytes/pixel.
                self.atlas.insert_rgba(
                    image.placement.width,
                    image.placement.height,
                    image.data.clone(),
                )
            }
        };
        let Some(atlas_rect) = atlas_rect else {
            // Atlas full — don't poison the cache; future frames may evict
            // and have room. For now leave as None so we don't infinitely
            // retry within one frame.
            self.cache.insert(hash, None);
            return None;
        };

        let cached = CachedGlyph {
            atlas: atlas_rect,
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            color: is_color,
        };
        self.cache.insert(hash, Some(cached));
        Some(cached)
    }
}

fn measure_buffer(buffer: &Buffer, size_px: f32) -> Vec2 {
    let mut max_x: f32 = 0.0;
    let mut max_baseline: f32 = 0.0;
    for run in buffer.layout_runs() {
        max_baseline = max_baseline.max(run.line_y);
        for glyph in run.glyphs.iter() {
            max_x = max_x.max(glyph.x + glyph.w);
        }
    }
    // Approximate height as the bottom of the last baseline plus a descent
    // allowance. Sufficient for single-line layouts.
    let height = (max_baseline + size_px * 0.35).max(size_px);
    Vec2::new(max_x.ceil(), height.ceil())
}

fn shape_key_hash(text: &str, size_px: f32, max_width: Option<f32>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = AHasher::default();
    text.hash(&mut h);
    size_px.to_bits().hash(&mut h);
    max_width.map(|w| w.to_bits()).hash(&mut h);
    h.finish()
}

fn cache_key_hash(key: &CacheKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = AHasher::default();
    key.font_id.hash(&mut h);
    key.glyph_id.hash(&mut h);
    key.font_size_bits.hash(&mut h);
    key.x_bin.hash(&mut h);
    key.y_bin.hash(&mut h);
    h.finish()
}
