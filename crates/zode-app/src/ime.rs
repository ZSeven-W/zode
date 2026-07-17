use jian_widgets::Rect;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::Window,
};

pub(crate) fn set_cursor_area(window: &Window, rect: Rect) {
    let (position, size) = physical_cursor_area(rect, window.scale_factor());
    window.set_ime_cursor_area(position, size);
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

fn physical_cursor_area(
    rect: Rect,
    scale_factor: f64,
) -> (PhysicalPosition<f64>, PhysicalSize<f64>) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (
        PhysicalPosition::new(
            f64::from(rect.origin.x) * scale,
            f64::from(rect.origin.y) * scale,
        ),
        PhysicalSize::new(
            f64::from(rect.size.x.max(1.0)) * scale,
            f64::from(rect.size.y.max(1.0)) * scale,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_area_uses_physical_scale_and_the_caret_top_left() {
        let (position, size) = physical_cursor_area(Rect::xywh(42.5, 720.0, 1.5, 17.0), 2.0);

        assert_eq!(position, PhysicalPosition::new(85.0, 1_440.0));
        assert_eq!(size, PhysicalSize::new(3.0, 34.0));
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
