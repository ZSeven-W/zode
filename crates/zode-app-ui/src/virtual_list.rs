use std::ops::Range;

const TAIL_EPSILON: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredItem {
    pub top: f32,
    pub bottom: f32,
}

impl MeasuredItem {
    pub fn new(top: f32, bottom: f32) -> Self {
        Self { top, bottom }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementCache {
    heights: Vec<f32>,
}

impl MeasurementCache {
    pub fn with_estimate(item_count: usize, estimate: f32) -> Self {
        Self {
            heights: vec![estimate.max(1.0); item_count],
        }
    }

    /// Replaces one cached height and returns its effect on total content
    /// height. Invalid indices or measurements are ignored.
    pub fn update(&mut self, index: usize, height: f32) -> Option<f32> {
        if !height.is_finite() || height <= 0.0 {
            return None;
        }
        let previous = self.heights.get_mut(index)?;
        let delta = height - *previous;
        *previous = height;
        Some(delta)
    }

    pub fn items(&self) -> Vec<MeasuredItem> {
        let mut top = 0.0;
        self.heights
            .iter()
            .map(|height| {
                let item = MeasuredItem::new(top, top + height);
                top = item.bottom;
                item
            })
            .collect()
    }

    pub fn total_height(&self) -> f32 {
        self.heights.iter().sum()
    }
}

/// Returns the transcript slice intersecting the viewport plus one item above
/// it, which prevents visible seams while fractional wheel deltas are applied.
pub fn visible_range(items: &[MeasuredItem], offset: f32, viewport_h: f32) -> Range<usize> {
    let start = items
        .partition_point(|item| item.bottom <= offset)
        .saturating_sub(1);
    let end = items
        .partition_point(|item| item.top < offset + viewport_h)
        .min(items.len());
    start..end.max(start)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualListState {
    offset: f32,
    follow_tail: bool,
}

impl VirtualListState {
    pub fn at_offset(offset: f32, follow_tail: bool) -> Self {
        Self {
            offset: offset.max(0.0),
            follow_tail,
        }
    }

    pub fn offset(self) -> f32 {
        self.offset
    }

    pub fn follows_tail(self) -> bool {
        self.follow_tail
    }

    pub fn content_grew(&mut self, previous_height: f32, next_height: f32) {
        if self.follow_tail {
            self.offset = (self.offset + (next_height - previous_height)).max(0.0);
        }
    }

    pub fn scroll_by(&mut self, delta: f32, max_offset: f32) {
        let max_offset = max_offset.max(0.0);
        self.offset = (self.offset + delta).clamp(0.0, max_offset);
        self.follow_tail = max_offset - self.offset <= TAIL_EPSILON;
    }
}

impl Default for VirtualListState {
    fn default() -> Self {
        Self::at_offset(0.0, true)
    }
}
