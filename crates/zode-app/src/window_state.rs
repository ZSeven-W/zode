use jian_widgets::Point2D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppWake {
    Redraw,
    Close,
}

#[derive(Debug, Clone, Copy)]
pub struct WindowState {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,
    pub cursor_logical: Point2D,
    pub dirty: bool,
}

impl WindowState {
    pub fn new(physical_width: u32, physical_height: u32, scale_factor: f64) -> Self {
        Self {
            physical_width: physical_width.max(1),
            physical_height: physical_height.max(1),
            scale_factor: valid_scale(scale_factor),
            cursor_logical: Point2D::ZERO,
            dirty: true,
        }
    }

    pub fn logical_size(self) -> (f32, f32) {
        (
            (f64::from(self.physical_width) / self.scale_factor) as f32,
            (f64::from(self.physical_height) / self.scale_factor) as f32,
        )
    }

    pub fn set_cursor_physical(&mut self, x: f64, y: f64) {
        self.cursor_logical = Point2D::new(
            (x / self.scale_factor) as f32,
            (y / self.scale_factor) as f32,
        );
    }
}

fn valid_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}
