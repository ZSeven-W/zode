use jian_widgets::Rect;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::Window,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct PhysicalCursorArea {
    position: PhysicalPosition<f64>,
    size: PhysicalSize<f64>,
}

/// Suppresses duplicate native IME anchor updates while preserving the
/// logical caret calculation performed by the UI layer.
#[derive(Debug, Default)]
struct ImeCursorAreaCache {
    last: Option<PhysicalCursorArea>,
}

impl ImeCursorAreaCache {
    fn update(&mut self, window: &Window, rect: Rect) -> bool {
        let Some(area) = self.changed_area(rect, window.scale_factor()) else {
            return false;
        };
        window.set_ime_cursor_area(area.position, area.size);
        true
    }

    /// Forces the next valid anchor through to the platform after state that
    /// can invalidate the native candidate-window placement changes.
    fn invalidate(&mut self) {
        self.last = None;
    }

    fn changed_area(&mut self, rect: Rect, scale_factor: f64) -> Option<PhysicalCursorArea> {
        let area = physical_cursor_area(rect, scale_factor);
        if self.last == Some(area) {
            return None;
        }
        self.last = Some(area);
        Some(area)
    }
}

/// Owns the measured composer anchor and its native submission cache.
#[derive(Debug)]
pub(crate) struct ImeState {
    cursor_area: Option<Rect>,
    cursor_area_dirty: bool,
    native_area: ImeCursorAreaCache,
}

impl Default for ImeState {
    fn default() -> Self {
        Self {
            cursor_area: None,
            cursor_area_dirty: true,
            native_area: ImeCursorAreaCache::default(),
        }
    }
}

impl ImeState {
    pub(crate) fn area(&self) -> Option<Rect> {
        self.cursor_area
    }

    pub(crate) fn needs_area_measurement(&self) -> bool {
        self.cursor_area_dirty || self.cursor_area.is_none()
    }

    pub(crate) fn set_area(&mut self, area: Rect) {
        self.cursor_area = Some(area);
        self.cursor_area_dirty = false;
    }

    pub(crate) fn mark_area_dirty(&mut self) {
        self.cursor_area_dirty = true;
    }

    pub(crate) fn update_native(&mut self, window: &Window, rect: Rect) -> bool {
        self.native_area.update(window, rect)
    }

    pub(crate) fn invalidate_native(&mut self) {
        self.native_area.invalidate();
    }
}

/// A non-zero caret-shaped anchor used before the first text paint has
/// measured the exact glyph position.
pub(crate) fn fallback_cursor_area(rect: Rect) -> Rect {
    let width = rect.size.x.max(1.0);
    let height = rect.size.y.max(1.0);
    let caret_width = 1.5_f32.min(width);
    let caret_height = 17.0_f32.min(height);
    let inset_x = 8.0_f32.min((width - caret_width).max(0.0));
    let inset_y = 6.0_f32.min((height - caret_height).max(0.0));
    Rect::xywh(
        rect.origin.x + inset_x,
        rect.origin.y + inset_y,
        caret_width,
        caret_height,
    )
}

fn physical_cursor_area(rect: Rect, scale_factor: f64) -> PhysicalCursorArea {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    PhysicalCursorArea {
        position: PhysicalPosition::new(
            f64::from(rect.origin.x) * scale,
            f64::from(rect.origin.y) * scale,
        ),
        size: PhysicalSize::new(
            f64::from(rect.size.x.max(1.0)) * scale,
            f64::from(rect.size.y.max(1.0)) * scale,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_area_uses_physical_scale_and_the_caret_top_left() {
        let area = physical_cursor_area(Rect::xywh(42.5, 720.0, 1.5, 17.0), 2.0);

        assert_eq!(area.position, PhysicalPosition::new(85.0, 1_440.0));
        assert_eq!(area.size, PhysicalSize::new(3.0, 34.0));
    }

    #[test]
    fn cursor_area_cache_suppresses_identical_physical_rectangles() {
        let mut cache = ImeCursorAreaCache::default();
        let rect = Rect::xywh(42.5, 720.0, 1.5, 17.0);

        assert!(cache.changed_area(rect, 2.0).is_some());
        assert!(cache.changed_area(rect, 2.0).is_none());
    }

    #[test]
    fn cursor_area_cache_tracks_physical_changes_and_invalidation() {
        let mut cache = ImeCursorAreaCache::default();
        let rect = Rect::xywh(42.5, 720.0, 1.5, 17.0);

        assert!(cache.changed_area(rect, 1.0).is_some());
        assert!(cache.changed_area(rect, 2.0).is_some());
        assert!(cache
            .changed_area(Rect::xywh(43.0, 720.0, 1.5, 17.0), 2.0)
            .is_some());
        assert!(cache
            .changed_area(Rect::xywh(43.0, 720.0, 1.5, 17.0), 2.0)
            .is_none());

        cache.invalidate();
        assert!(cache
            .changed_area(Rect::xywh(43.0, 720.0, 1.5, 17.0), 2.0)
            .is_some());
    }

    #[test]
    fn fallback_is_valid_before_the_first_preedit_frame() {
        let input = Rect::xywh(40.0, 700.0, 500.0, 120.0);
        let fallback = fallback_cursor_area(input);

        assert!(fallback.size.x > 0.0);
        assert!(fallback.size.y > 0.0);
        assert!(fallback.origin.x >= input.origin.x);
        assert!(fallback.origin.y >= input.origin.y);
        assert!(fallback.origin.x + fallback.size.x <= input.origin.x + input.size.x);
        assert!(fallback.origin.y + fallback.size.y <= input.origin.y + input.size.y);

        let compact = Rect::xywh(4.0, 8.0, 1.0, 1.0);
        assert_eq!(fallback_cursor_area(compact), compact);
    }
}
