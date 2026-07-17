use zode_app::{
    window_bootstrap::{hidden_window_attributes_for_placement, StartupWindowPlacement},
    window_state::{MonitorWorkArea, WindowGeometry},
    work_area::{
        resolve_startup_placement, restored_geometry_for_startup, PlatformWorkAreaProvider,
        WorkAreaError, WorkAreaProvider,
    },
};

#[derive(Clone)]
struct FixedWorkAreas(Result<Vec<MonitorWorkArea>, WorkAreaError>);

impl WorkAreaProvider for FixedWorkAreas {
    fn work_areas(&self) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        self.0.clone()
    }
}

#[test]
fn startup_restore_uses_provider_work_area_instead_of_full_monitor_bounds() {
    let saved = WindowGeometry {
        x: 40,
        y: 0,
        width: 1200,
        height: 1050,
        maximized: false,
    };
    let provider = FixedWorkAreas(Ok(vec![MonitorWorkArea {
        x: 0,
        y: 25,
        width: 1920,
        height: 1055,
    }]));

    let restored = restored_geometry_for_startup(Some(saved), &provider)
        .expect("the fixed work area is available")
        .expect("saved geometry should be restored");

    assert_eq!(restored.y, 25);
    assert_eq!(restored.height, 1050);
}

#[test]
fn unavailable_work_area_is_explicit_so_startup_can_avoid_saved_position() {
    let saved = WindowGeometry {
        x: 4000,
        y: 3000,
        width: 1200,
        height: 900,
        maximized: true,
    };
    let provider = FixedWorkAreas(Err(WorkAreaError::Unsupported {
        platform: "wayland",
        reason: "the compositor does not expose monitor work areas",
    }));

    assert!(matches!(
        restored_geometry_for_startup(Some(saved), &provider),
        Err(WorkAreaError::Unsupported {
            platform: "wayland",
            ..
        })
    ));
}

#[test]
fn no_saved_geometry_does_not_require_a_platform_query() {
    let provider = FixedWorkAreas(Err(WorkAreaError::QueryFailed(
        "the provider must not be called",
    )));

    assert_eq!(restored_geometry_for_startup(None, &provider), Ok(None));
}

#[test]
fn desktop_startup_does_not_mislabel_winit_monitor_bounds_as_work_areas() {
    let source = include_str!("../src/app.rs");
    let platform = include_str!("../src/platform_work_area.rs");
    let manifest = include_str!("../Cargo.toml");
    let casement_windows = include_str!("../../../vendor/casement/src/platform/windows.rs");

    assert!(!source.contains("available_monitors()"));
    assert!(source.contains("PlatformWorkAreaProvider"));
    assert!(platform.contains("visibleFrame"));
    assert!(platform.contains("native_id"));
    assert!(platform.contains("MonitorHandleExtWindows"));
    assert!(platform.contains("monitor.work_area()"));
    assert!(!platform.contains("windows_sys"));
    assert!(!platform.contains("unsafe"));
    assert!(!manifest.contains("windows-sys"));
    assert!(casement_windows
        .contains("fn work_area(&self) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>;"));
    assert!(platform.contains("_NET_WORKAREA"));
    assert!(platform.contains("randr_get_monitors"));

    let _provider_type = std::any::TypeId::of::<PlatformWorkAreaProvider>();
}

#[cfg(target_os = "windows")]
fn windows_monitor_work_area_extension_has_safe_optional_geometry(
    monitor: &winit::monitor::MonitorHandle,
) {
    use winit::{
        dpi::{PhysicalPosition, PhysicalSize},
        platform::windows::MonitorHandleExtWindows,
    };

    let _: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> = monitor.work_area();
}

#[test]
fn unavailable_backend_preserves_size_and_maximize_but_omits_position() {
    let saved = WindowGeometry {
        x: -1900,
        y: 32,
        width: 1180,
        height: 820,
        maximized: true,
    };

    let provider = FixedWorkAreas(Err(WorkAreaError::Unsupported {
        platform: "Wayland",
        reason: "the compositor protocol does not expose monitor work areas",
    }));
    let (placement, warning) = resolve_startup_placement(Some(saved), &provider);
    let attributes = hidden_window_attributes_for_placement(placement);

    assert_eq!(
        attributes.inner_size,
        Some(winit::dpi::Size::Physical(winit::dpi::PhysicalSize::new(
            1180, 820,
        ))),
    );
    assert!(attributes.position.is_none());
    assert_eq!(placement, StartupWindowPlacement::Unpositioned(saved));
    assert!(placement.maximized());
    assert!(matches!(
        warning,
        Some(WorkAreaError::Unsupported {
            platform: "Wayland",
            ..
        })
    ));
}
