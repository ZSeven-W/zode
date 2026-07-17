use std::{
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    hash::{Hash, Hasher},
};

use jian_core::render::{TextAlign, TextRun};
use skia_safe::Picture;

const MAX_ENTRIES: usize = 512;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
struct TextPictureKey {
    content: Box<str>,
    font_family: Box<str>,
    font_size_bits: u32,
    font_weight: u16,
    color: u32,
    max_width_bits: u32,
    align: u8,
    line_height_bits: u32,
    dpi_bits: u32,
    origin_x_phase_bits: u32,
    origin_y_phase_bits: u32,
}

impl TextPictureKey {
    fn from_run(run: &TextRun, dpi: f32) -> Self {
        Self {
            content: run.content.clone().into_boxed_str(),
            font_family: run.font_family.clone().into_boxed_str(),
            font_size_bits: run.font_size.to_bits(),
            font_weight: run.font_weight,
            color: run.color.0,
            max_width_bits: run.max_width.to_bits(),
            align: align_key(run.align),
            line_height_bits: run.line_height.to_bits(),
            dpi_bits: dpi.to_bits(),
            origin_x_phase_bits: device_phase_bits(run.origin.x, dpi),
            origin_y_phase_bits: device_phase_bits(run.origin.y, dpi),
        }
    }

    fn matches(&self, run: &TextRun, dpi: f32) -> bool {
        self.content.as_ref() == run.content
            && self.font_family.as_ref() == run.font_family
            && self.font_size_bits == run.font_size.to_bits()
            && self.font_weight == run.font_weight
            && self.color == run.color.0
            && self.max_width_bits == run.max_width.to_bits()
            && self.align == align_key(run.align)
            && self.line_height_bits == run.line_height.to_bits()
            && self.dpi_bits == dpi.to_bits()
            && self.origin_x_phase_bits == device_phase_bits(run.origin.x, dpi)
            && self.origin_y_phase_bits == device_phase_bits(run.origin.y, dpi)
    }
}

struct CachedTextPicture {
    id: u64,
    key: TextPictureKey,
    picture: Picture,
    origin_x: f32,
    origin_y: f32,
    bytes: usize,
}

/// Host-owned display-list cache for text whose shaping inputs are unchanged.
///
/// Device-space subpixel origin phases participate in the key. Pictures keep
/// their recorded origin and may only be translated to a matching phase.
pub(super) struct TextPictureCache {
    buckets: HashMap<u64, Vec<CachedTextPicture>>,
    insertion_order: VecDeque<(u64, u64)>,
    entries: usize,
    bytes: usize,
    next_id: u64,
    font_generation: u64,
    #[cfg(test)]
    hits: usize,
    #[cfg(test)]
    misses: usize,
    #[cfg(test)]
    invalidations: usize,
}

impl TextPictureCache {
    pub(super) fn new() -> Self {
        Self::with_generation(jian_skia::font_generation())
    }

    fn with_generation(font_generation: u64) -> Self {
        Self {
            buckets: HashMap::new(),
            insertion_order: VecDeque::new(),
            entries: 0,
            bytes: 0,
            next_id: 0,
            font_generation,
            #[cfg(test)]
            hits: 0,
            #[cfg(test)]
            misses: 0,
            #[cfg(test)]
            invalidations: 0,
        }
    }

    pub(super) fn get(&mut self, run: &TextRun, dpi: f32) -> Option<(Picture, f32, f32)> {
        self.get_with_generation(run, dpi, jian_skia::font_generation())
    }

    fn get_with_generation(
        &mut self,
        run: &TextRun,
        dpi: f32,
        generation: u64,
    ) -> Option<(Picture, f32, f32)> {
        self.sync_generation(generation);
        let hash = hash_run(run, dpi);
        let picture = self.buckets.get(&hash).and_then(|bucket| {
            bucket
                .iter()
                .find(|cached| cached.key.matches(run, dpi))
                .map(|cached| (cached.picture.clone(), cached.origin_x, cached.origin_y))
        });
        #[cfg(test)]
        if picture.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        picture
    }

    pub(super) fn insert(&mut self, run: &TextRun, dpi: f32, picture: Picture) {
        let bytes = picture.approximate_bytes_used();
        if bytes > MAX_BYTES {
            return;
        }
        while self.entries >= MAX_ENTRIES || self.bytes.saturating_add(bytes) > MAX_BYTES {
            if !self.evict_oldest() {
                return;
            }
        }

        let hash = hash_run(run, dpi);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.buckets
            .entry(hash)
            .or_default()
            .push(CachedTextPicture {
                id,
                key: TextPictureKey::from_run(run, dpi),
                picture,
                origin_x: run.origin.x,
                origin_y: run.origin.y,
                bytes,
            });
        self.insertion_order.push_back((hash, id));
        self.entries += 1;
        self.bytes += bytes;
    }

    fn sync_generation(&mut self, generation: u64) {
        if generation == self.font_generation {
            return;
        }
        self.clear();
        self.font_generation = generation;
        #[cfg(test)]
        {
            self.invalidations += 1;
        }
    }

    fn evict_oldest(&mut self) -> bool {
        let Some((hash, id)) = self.insertion_order.pop_front() else {
            return false;
        };
        let Some(bucket) = self.buckets.get_mut(&hash) else {
            return false;
        };
        let Some(index) = bucket.iter().position(|cached| cached.id == id) else {
            return false;
        };
        let removed = bucket.swap_remove(index);
        self.entries -= 1;
        self.bytes -= removed.bytes;
        if bucket.is_empty() {
            self.buckets.remove(&hash);
        }
        true
    }

    fn clear(&mut self) {
        self.buckets.clear();
        self.insertion_order.clear();
        self.entries = 0;
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(super) fn stats(&self) -> (usize, usize, usize, usize) {
        (self.hits, self.misses, self.invalidations, self.entries)
    }
}

fn hash_run(run: &TextRun, dpi: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    run.content.hash(&mut hasher);
    run.font_family.hash(&mut hasher);
    run.font_size.to_bits().hash(&mut hasher);
    run.font_weight.hash(&mut hasher);
    run.color.0.hash(&mut hasher);
    run.max_width.to_bits().hash(&mut hasher);
    align_key(run.align).hash(&mut hasher);
    run.line_height.to_bits().hash(&mut hasher);
    dpi.to_bits().hash(&mut hasher);
    device_phase_bits(run.origin.x, dpi).hash(&mut hasher);
    device_phase_bits(run.origin.y, dpi).hash(&mut hasher);
    hasher.finish()
}

fn device_phase_bits(value: f32, dpi: f32) -> u32 {
    let device_value = value * dpi;
    if !device_value.is_finite() {
        return device_value.to_bits();
    }
    let phase = device_value - device_value.floor();
    if phase == 0.0 {
        0.0_f32.to_bits()
    } else {
        phase.to_bits()
    }
}

fn align_key(align: TextAlign) -> u8 {
    match align {
        TextAlign::Start => 0,
        TextAlign::Center => 1,
        TextAlign::End => 2,
    }
}

#[cfg(test)]
mod tests {
    use jian_core::{
        geometry::Point,
        render::{TextAlign, TextRun},
        scene::Color,
    };
    use skia_safe::{Picture, Rect};

    use super::{TextPictureCache, MAX_ENTRIES};

    fn run(content: &str) -> TextRun {
        TextRun {
            content: content.to_owned(),
            font_family: "system-ui".to_owned(),
            font_size: 14.0,
            font_weight: 400,
            color: Color::rgba(10, 20, 30, 255),
            origin: Point::new(4.0, 8.0),
            max_width: 220.0,
            align: TextAlign::Start,
            line_height: 1.3,
        }
    }

    fn picture() -> Picture {
        Picture::new_placeholder(Rect::from_xywh(0.0, 0.0, 10.0, 10.0))
    }

    #[test]
    fn integer_device_translation_is_ignored_but_phase_and_visual_properties_are_keyed() {
        let mut cache = TextPictureCache::with_generation(7);
        let base = run("cached");
        cache.insert(&base, 1.5, picture());

        let mut moved = base.clone();
        moved.origin = Point::new(6.0, 10.0);
        assert!(cache.get_with_generation(&moved, 1.5, 7).is_some());

        let variants = [
            run("different"),
            TextRun {
                font_family: "ui-monospace".to_owned(),
                ..base.clone()
            },
            TextRun {
                font_size: 15.0,
                ..base.clone()
            },
            TextRun {
                font_weight: 600,
                ..base.clone()
            },
            TextRun {
                color: Color::rgba(30, 20, 10, 255),
                ..base.clone()
            },
            TextRun {
                max_width: 240.0,
                ..base.clone()
            },
            TextRun {
                align: TextAlign::Center,
                ..base.clone()
            },
            TextRun {
                line_height: 1.5,
                ..base.clone()
            },
            TextRun {
                origin: Point::new(4.25, 8.0),
                ..base
            },
        ];
        for variant in &variants {
            assert!(cache.get_with_generation(variant, 1.5, 7).is_none());
        }
        assert_eq!(cache.stats(), (1, variants.len(), 0, 1));
    }

    #[test]
    fn font_generation_change_invalidates_cached_pictures() {
        let mut cache = TextPictureCache::with_generation(4);
        let run = run("generation");
        cache.insert(&run, 1.0, picture());
        assert!(cache.get_with_generation(&run, 1.0, 4).is_some());

        assert!(cache.get_with_generation(&run, 1.0, 5).is_none());

        assert_eq!(cache.stats(), (1, 1, 1, 0));
    }

    #[test]
    fn entry_limit_evicts_oldest_picture() {
        let mut cache = TextPictureCache::with_generation(9);
        for index in 0..=MAX_ENTRIES {
            cache.insert(&run(&format!("entry-{index}")), 1.0, picture());
        }

        assert_eq!(cache.entries, MAX_ENTRIES);
        assert!(cache.get_with_generation(&run("entry-0"), 1.0, 9).is_none());
        assert!(cache
            .get_with_generation(&run(&format!("entry-{MAX_ENTRIES}")), 1.0, 9)
            .is_some());
    }
}
