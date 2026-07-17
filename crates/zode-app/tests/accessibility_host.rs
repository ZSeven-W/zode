use zode_app::accessibility_host::{
    post_install_window_actions, AccessibilityBridge, AccessibilityHost, PostInstallWindowAction,
};

fn assert_bridge_contract<T: AccessibilityBridge>() {}

#[test]
fn platform_accessibility_host_has_update_and_focus_action_hooks() {
    assert_bridge_contract::<AccessibilityHost>();
    let _: fn(
        &winit::window::Window,
        accesskit::TreeUpdate,
        zode_app::accessibility_host::AccessibilityWake,
    )
        -> Result<AccessibilityHost, zode_app::accessibility_host::AccessibilityHostError> =
        AccessibilityHost::install;
}

#[test]
fn linux_adapter_receives_initial_move_and_resize_bounds() {
    let host = include_str!("../src/accessibility_host.rs");
    let app = include_str!("../src/app.rs");
    let window = include_str!("../src/app/window.rs");

    assert!(host.contains("adapter.set_root_window_bounds"));
    assert!(host.contains("host.update_window_bounds(window)"));
    assert!(app.contains("WindowEvent::Moved(_)"));
    assert!(app.contains("self.update_accessibility_window_bounds()"));
    assert!(app.contains("self.window_metrics_update_pending = true"));
    assert!(window.contains("if self.window_metrics_update_pending"));
    assert!(window.contains("self.update_accessibility_window_bounds()"));
}

#[test]
fn accessibility_adapter_is_installed_before_the_window_is_visible() {
    assert_eq!(
        post_install_window_actions(false),
        [PostInstallWindowAction::Show],
    );
    assert_eq!(
        post_install_window_actions(true),
        [
            PostInstallWindowAction::Maximize,
            PostInstallWindowAction::Show,
        ],
    );
    let source = include_str!("../src/accessibility_host.rs");
    let window_source = include_str!("../src/window_bootstrap.rs");
    let helper = source
        .split("pub fn install_before_show")
        .nth(1)
        .expect("real bootstrap helper exists");
    assert!(helper.find("Self::install").unwrap() < helper.find("for action").unwrap());
    assert!(helper.contains("window.set_maximized(true)"));
    assert!(helper.contains("window.set_visible(true)"));
    assert!(window_source.contains(".with_visible(false)"));
    assert!(include_str!("../src/app/window.rs").contains("AccessibilityHost::install_before_show"));
}

#[test]
fn unsafe_is_confined_to_the_explicit_platform_adapter_module() {
    let crate_root = include_str!("../src/lib.rs");
    let adapter = include_str!("../src/accessibility_host.rs");

    assert!(crate_root.contains("#![deny(unsafe_code)]"));
    assert!(crate_root.contains("#[allow(unsafe_code)]\npub mod accessibility_host;"));
    assert_eq!(
        crate_root.matches("#[allow(unsafe_code)]").count(),
        1,
        "only the native accessibility adapter may opt out of the crate unsafe ban",
    );
    assert!(adapter.contains("accesskit_macos::SubclassingAdapter::new"));
}
