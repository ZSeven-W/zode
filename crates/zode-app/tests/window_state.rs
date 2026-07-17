use zode_app::window_state::{
    restore_window_geometry, update_window_geometry, MonitorWorkArea, WindowGeometry,
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
};

#[test]
fn restore_preserves_visible_geometry_on_a_negative_coordinate_monitor() {
    let saved = WindowGeometry {
        x: -1800,
        y: 60,
        width: 1200,
        height: 900,
        maximized: false,
    };
    let displays = [MonitorWorkArea {
        x: -1920,
        y: 0,
        width: 1920,
        height: 1080,
    }];

    assert_eq!(restore_window_geometry(Some(saved), &displays), saved);
}

#[test]
fn restore_clamps_completely_offscreen_and_oversize_windows_to_work_areas() {
    let display = MonitorWorkArea {
        x: -1920,
        y: 24,
        width: 1920,
        height: 1056,
    };
    let offscreen = WindowGeometry {
        x: 4000,
        y: 3000,
        width: 1000,
        height: 800,
        maximized: false,
    };
    let restored = restore_window_geometry(Some(offscreen), &[display]);
    assert!(restored.x >= display.x);
    assert!(restored.y >= display.y);
    assert!(restored.x + restored.width as i32 <= display.x + display.width as i32);
    assert!(restored.y + restored.height as i32 <= display.y + display.height as i32);

    let oversize = WindowGeometry {
        x: -2500,
        y: -500,
        width: 4000,
        height: 2400,
        maximized: true,
    };
    let restored = restore_window_geometry(Some(oversize), &[display]);
    assert_eq!((restored.width, restored.height), (1920, 1056));
    assert_eq!((restored.x, restored.y), (-1920, 24));
    assert!(restored.maximized);
}

#[test]
fn restore_without_display_information_uses_a_safe_default() {
    let saved = WindowGeometry {
        x: i32::MAX,
        y: i32::MIN,
        width: u32::MAX,
        height: u32::MAX,
        maximized: true,
    };

    assert_eq!(
        restore_window_geometry(Some(saved), &[]),
        WindowGeometry {
            x: 0,
            y: 0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            maximized: false,
        },
    );
}

#[test]
fn maximized_notifications_keep_the_last_normal_geometry() {
    let mut saved = WindowGeometry {
        x: 100,
        y: 80,
        width: 1221,
        height: 992,
        maximized: false,
    };
    update_window_geometry(
        &mut saved,
        WindowGeometry {
            x: 0,
            y: 0,
            width: 1800,
            height: 1080,
            maximized: true,
        },
        true,
    );

    assert_eq!((saved.x, saved.y), (100, 80));
    assert_eq!((saved.width, saved.height), (1221, 992));
    assert!(saved.maximized);
}
