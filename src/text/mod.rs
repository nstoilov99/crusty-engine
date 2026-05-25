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
}

pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: GlyphAtlas,
    cache: HashMap<u64, Option<CachedGlyph>, std::hash::BuildHasherDefault<AHasher>>,
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
        }
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
        let line_height = (size_px * 1.3).max(size_px + 2.0);
        let metrics = Metrics::new(size_px, line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new(),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut max_x: f32 = 0.0;
        let mut max_baseline: f32 = 0.0;
        for run in buffer.layout_runs() {
            max_baseline = max_baseline.max(run.line_y);
            for glyph in run.glyphs.iter() {
                max_x = max_x.max(glyph.x + glyph.w);
            }
        }
        // Approximate height as the bottom of the last baseline plus a
        // descent allowance. Sufficient for single-line layouts; for
        // multi-line we'd want `line_y + line_height` of the last run.
        let height = (max_baseline + size_px * 0.35).max(size_px);
        Vec2::new(max_x.ceil(), height.ceil())
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
        let line_height = (size_px * 1.3).max(size_px + 2.0);
        let metrics = Metrics::new(size_px, line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new(),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        for run in buffer.layout_runs() {
            let baseline_y = position.y + run.line_y;
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((position.x, baseline_y), 1.0);
                let key = physical.cache_key;
                let cached = self.ensure_cached(key);
                let Some(cached) = cached else {
                    continue;
                };

                let quad_pos = Pos2::new(
                    physical.x as f32 + cached.left as f32,
                    physical.y as f32 - cached.top as f32,
                );
                let quad_size = Vec2::new(cached.width as f32, cached.height as f32);
                if quad_size.x <= 0.0 || quad_size.y <= 0.0 {
                    continue;
                }
                paint.push(PaintCmd::Glyph {
                    rect: Rect::from_min_size(quad_pos, quad_size),
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

        // We only handle alpha-mask content. Color glyphs (emoji) and
        // subpixel-RGB glyphs are skipped for Phase 2; the atlas is R8.
        let bytes: Vec<u8> = match image.content {
            SwashContent::Mask => image.data.clone(),
            SwashContent::SubpixelMask => {
                // SubpixelMask stores 3 sub-channels per pixel; collapse to
                // luminance by averaging.
                image
                    .data
                    .chunks_exact(3)
                    .map(|c| ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8)
                    .collect()
            }
            SwashContent::Color => {
                self.cache.insert(hash, None);
                return None;
            }
        };


        let atlas_rect = self.atlas.insert(image.placement.width, image.placement.height, bytes);
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
        };
        self.cache.insert(hash, Some(cached));
        Some(cached)
    }
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
