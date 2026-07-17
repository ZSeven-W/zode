use casement::window::ResizeDirection;
use jian_widgets::Point2D;
use zode_app::event_map::{is_drag_region, resize_direction};
use zode_app_ui::{Insets, WorkspaceLayout};

#[test]
fn borderless_edges_map_to_resize_directions() {
    assert_eq!(
        resize_direction(0.0, 400.0, 1200.0, 800.0),
        Some(ResizeDirection::West),
    );
    assert_eq!(
        resize_direction(1200.0, 0.0, 1200.0, 800.0),
        Some(ResizeDirection::NorthEast),
    );
    assert_eq!(
        resize_direction(600.0, 800.0, 1200.0, 800.0),
        Some(ResizeDirection::South),
    );
    assert_eq!(resize_direction(600.0, 400.0, 1200.0, 800.0), None);
}

#[test]
fn invalid_window_geometry_never_requests_resize() {
    assert_eq!(resize_direction(0.0, 0.0, 0.0, 800.0), None);
    assert_eq!(resize_direction(f32::NAN, 0.0, 1200.0, 800.0), None);
}

#[test]
fn title_drag_excludes_interactive_controls() {
    let geometry = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);
    assert!(is_drag_region(Point2D::new(600.0, 20.0), &geometry));
    assert!(!is_drag_region(Point2D::new(1080.0, 20.0), &geometry));
    assert!(!is_drag_region(Point2D::new(600.0, 80.0), &geometry));
}
