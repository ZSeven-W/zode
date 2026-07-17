use zode_app_model::LayoutClass;
use zode_app_ui::{
    Insets, RectExt, WorkspaceLayout, WorkspaceLayoutOptions, COMPOSER_BOTTOM, CONTENT_W, SIDEBAR_W,
};

#[test]
fn phone_layout_hides_sidebar_and_keeps_composer_above_safe_area() {
    let geometry = WorkspaceLayout::compute(
        390.0,
        844.0,
        Insets {
            top: 47.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        },
    );

    assert_eq!(geometry.class, LayoutClass::Phone);
    assert_eq!(geometry.sidebar.width(), 0.0);
    assert!(geometry.composer.max_y() <= 844.0 - 34.0);
    assert!(geometry.transcript.max_y() <= geometry.composer.min_y());
}

#[test]
fn extreme_and_invalid_insets_never_place_regions_outside_viewport() {
    for insets in [
        Insets {
            top: 700.0,
            right: 300.0,
            bottom: 700.0,
            left: 300.0,
        },
        Insets {
            top: f32::INFINITY,
            right: f32::NAN,
            bottom: -20.0,
            left: 500.0,
        },
    ] {
        let geometry = WorkspaceLayout::compute(390.0, 844.0, insets);
        for rect in [
            geometry.sidebar,
            geometry.top_bar,
            geometry.transcript,
            geometry.composer,
            geometry.context_panel,
        ] {
            assert!(rect.min_x().is_finite() && rect.min_y().is_finite());
            assert!(rect.width().is_finite() && rect.height().is_finite());
            assert!(rect.min_x() >= 0.0 && rect.min_y() >= 0.0);
            assert!(rect.max_x() <= 390.0 && rect.max_y() <= 844.0);
            assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
        }
    }
}

#[test]
fn desktop_reference_geometry_keeps_the_existing_visual_rhythm() {
    let geometry = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);

    assert_eq!(geometry.class, LayoutClass::Wide);
    assert_eq!(geometry.sidebar.width(), SIDEBAR_W);
    assert_eq!(geometry.transcript.width(), CONTENT_W);
    assert_eq!(geometry.composer.width(), CONTENT_W);
    assert_eq!(geometry.composer.max_y(), 992.0 - COMPOSER_BOTTOM);
}

#[test]
fn optional_context_panel_does_not_squeeze_the_centered_wide_conversation() {
    let without_panel = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let with_panel = WorkspaceLayout::compute_with_options(
        1800.0,
        1080.0,
        Insets::ZERO,
        WorkspaceLayoutOptions {
            context_panel_width: 300.0,
        },
    );

    assert_eq!(without_panel.sidebar.width(), 240.0);
    assert_eq!(without_panel.transcript.width(), 736.0);
    assert_eq!(with_panel.transcript, without_panel.transcript);
    assert_eq!(with_panel.composer, without_panel.composer);
    assert_eq!(with_panel.context_panel.width(), 300.0);
    assert!(with_panel.context_panel.min_x() >= with_panel.composer.max_x());
}
