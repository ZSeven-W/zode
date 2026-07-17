use jian_widgets::Rect;
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    window::Window,
};

pub(crate) fn set_cursor_area(window: &Window, rect: Rect) {
    let (position, size) = logical_cursor_area(rect);
    window.set_ime_cursor_area(position, size);
}

fn logical_cursor_area(rect: Rect) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    (
        LogicalPosition::new(f64::from(rect.origin.x), f64::from(rect.origin.y)),
        LogicalSize::new(f64::from(rect.size.x), f64::from(rect.size.y)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_area_preserves_logical_composer_geometry() {
        let (position, size) = logical_cursor_area(Rect::xywh(42.5, 720.0, 1.5, 17.0));

        assert_eq!(position, LogicalPosition::new(42.5, 720.0));
        assert_eq!(size, LogicalSize::new(1.5, 17.0));
    }
}
