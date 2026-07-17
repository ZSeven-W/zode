use jian_widgets::Point2D;
pub use zode_app_runtime::WindowGeometry;
use zode_app_ui::Insets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppWake {
    Redraw,
    Close,
}

pub const DEFAULT_WINDOW_WIDTH: u32 = 1221;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 992;
pub const MIN_WINDOW_WIDTH: f64 = 320.0;
pub const MIN_WINDOW_HEIGHT: f64 = 480.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorWorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn restore_window_geometry(
    saved: Option<WindowGeometry>,
    work_areas: &[MonitorWorkArea],
) -> WindowGeometry {
    let fallback = default_window_geometry();
    let Some(work_area) = select_work_area(saved.unwrap_or(fallback), work_areas) else {
        return fallback;
    };
    clamp_to_work_area(saved.unwrap_or(fallback), work_area)
}

/// Records a platform geometry notification while retaining windowed bounds.
pub fn update_window_geometry(
    saved: &mut WindowGeometry,
    reported: WindowGeometry,
    maximized: bool,
    minimized: bool,
) {
    if minimized || reported.width == 0 || reported.height == 0 {
        return;
    }
    if maximized {
        saved.maximized = true;
    } else {
        *saved = WindowGeometry {
            maximized: false,
            ..reported
        };
    }
}

fn default_window_geometry() -> WindowGeometry {
    WindowGeometry {
        x: 0,
        y: 0,
        width: DEFAULT_WINDOW_WIDTH,
        height: DEFAULT_WINDOW_HEIGHT,
        maximized: false,
    }
}

fn select_work_area(
    geometry: WindowGeometry,
    work_areas: &[MonitorWorkArea],
) -> Option<MonitorWorkArea> {
    let work_areas = work_areas
        .iter()
        .copied()
        .filter(|area| area.width > 0 && area.height > 0);

    work_areas.max_by(|left, right| {
        let left_overlap = intersection_area(geometry, *left);
        let right_overlap = intersection_area(geometry, *right);
        left_overlap.cmp(&right_overlap).then_with(|| {
            distance_to_work_area(geometry, *right).cmp(&distance_to_work_area(geometry, *left))
        })
    })
}

fn intersection_area(geometry: WindowGeometry, work_area: MonitorWorkArea) -> i128 {
    let left = i128::from(geometry.x).max(i128::from(work_area.x));
    let top = i128::from(geometry.y).max(i128::from(work_area.y));
    let right = (i128::from(geometry.x) + i128::from(geometry.width))
        .min(i128::from(work_area.x) + i128::from(work_area.width));
    let bottom = (i128::from(geometry.y) + i128::from(geometry.height))
        .min(i128::from(work_area.y) + i128::from(work_area.height));
    (right - left).max(0) * (bottom - top).max(0)
}

fn distance_to_work_area(geometry: WindowGeometry, work_area: MonitorWorkArea) -> i128 {
    let center_x = i128::from(geometry.x) * 2 + i128::from(geometry.width);
    let center_y = i128::from(geometry.y) * 2 + i128::from(geometry.height);
    let left = i128::from(work_area.x) * 2;
    let top = i128::from(work_area.y) * 2;
    let right = left + i128::from(work_area.width) * 2;
    let bottom = top + i128::from(work_area.height) * 2;
    let dx = distance_to_range(center_x, left, right);
    let dy = distance_to_range(center_y, top, bottom);
    dx * dx + dy * dy
}

fn distance_to_range(value: i128, start: i128, end: i128) -> i128 {
    if value < start {
        start - value
    } else if value > end {
        value - end
    } else {
        0
    }
}

fn clamp_to_work_area(geometry: WindowGeometry, work_area: MonitorWorkArea) -> WindowGeometry {
    let width = geometry.width.max(1).min(work_area.width);
    let height = geometry.height.max(1).min(work_area.height);
    let min_x = i64::from(work_area.x);
    let min_y = i64::from(work_area.y);
    let max_x = min_x + i64::from(work_area.width - width);
    let max_y = min_y + i64::from(work_area.height - height);

    WindowGeometry {
        x: clamp_i32(i64::from(geometry.x), min_x, max_x),
        y: clamp_i32(i64::from(geometry.y), min_y, max_y),
        width,
        height,
        maximized: geometry.maximized,
    }
}

fn clamp_i32(value: i64, min: i64, max: i64) -> i32 {
    value
        .clamp(min, max)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[derive(Debug, Clone, Copy)]
pub struct WindowState {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,
    pub cursor_logical: Point2D,
    pub safe_area_insets: Insets,
    pub primary_sidebar_resize_active: bool,
    pub secondary_sidebar_resize_active: bool,
    pub snapshot_scroll_only_invalid: bool,
    pub dirty: bool,
}

impl WindowState {
    pub fn new(physical_width: u32, physical_height: u32, scale_factor: f64) -> Self {
        Self {
            physical_width: physical_width.max(1),
            physical_height: physical_height.max(1),
            scale_factor: valid_scale(scale_factor),
            cursor_logical: Point2D::ZERO,
            safe_area_insets: Insets::ZERO,
            primary_sidebar_resize_active: false,
            secondary_sidebar_resize_active: false,
            snapshot_scroll_only_invalid: false,
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

    pub fn set_safe_area_insets(&mut self, insets: Insets) {
        self.safe_area_insets = Insets {
            top: finite_non_negative(insets.top),
            right: finite_non_negative(insets.right),
            bottom: finite_non_negative(insets.bottom),
            left: finite_non_negative(insets.left),
        };
    }
}

fn valid_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
