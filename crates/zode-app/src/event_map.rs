use jian_widgets::Point2D;
use winit::window::ResizeDirection;
use zode_app_ui::WorkspaceLayout;

const RESIZE_RING: f32 = 6.0;
const WINDOW_CONTROLS_WIDTH: f32 = 160.0;

pub fn resize_direction(x: f32, y: f32, width: f32, height: f32) -> Option<ResizeDirection> {
    if ![x, y, width, height].iter().all(|value| value.is_finite())
        || width <= RESIZE_RING * 2.0
        || height <= RESIZE_RING * 2.0
    {
        return None;
    }
    let west = x <= RESIZE_RING;
    let east = x >= width - RESIZE_RING;
    let north = y <= RESIZE_RING;
    let south = y >= height - RESIZE_RING;
    match (west, east, north, south) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

/// The non-interactive center of the thread header is the native drag surface.
pub fn is_drag_region(point: Point2D, geometry: &WorkspaceLayout) -> bool {
    let header = geometry.top_bar;
    header.contains(point)
        && point.x >= header.origin.x + 48.0
        && point.x < header.origin.x + header.size.x - WINDOW_CONTROLS_WIDTH
}
