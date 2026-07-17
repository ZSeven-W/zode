use zode_app_model::LayoutClass;
use zode_app_ui::{Insets, RectExt, WorkspaceLayout, CONTENT_W, SIDEBAR_W, TOP_BAR_H};

#[test]
fn reference_layout_matches_codex_rhythm() {
    let geometry = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);

    assert_eq!(geometry.class, LayoutClass::Wide);
    assert_eq!(geometry.sidebar.width(), SIDEBAR_W);
    assert_eq!(geometry.top_bar.height(), TOP_BAR_H);
    assert!((geometry.transcript.width() - CONTENT_W).abs() <= 2.0);
    assert!((geometry.composer.max_y() - 978.0).abs() <= 2.0);
    assert_eq!(geometry.composer.width(), geometry.transcript.width());
}

#[test]
fn responsive_classes_keep_content_inside_safe_viewport() {
    let compact = WorkspaceLayout::compute(900.0, 700.0, Insets::ZERO);
    assert_eq!(compact.class, LayoutClass::Compact);
    assert_eq!(compact.sidebar.width(), 64.0);
    assert!(compact.composer.min_x() >= compact.sidebar.max_x());

    let phone = WorkspaceLayout::compute(
        600.0,
        800.0,
        Insets {
            top: 18.0,
            right: 8.0,
            bottom: 24.0,
            left: 8.0,
        },
    );
    assert_eq!(phone.class, LayoutClass::Phone);
    assert_eq!(phone.sidebar.width(), 0.0);
    assert!(phone.composer.min_x() >= 8.0);
    assert!(phone.composer.max_x() <= 592.0);
    assert!(phone.top_bar.min_y() >= 18.0);
    assert!(phone.composer.max_y() <= 776.0);
    assert!(phone.transcript.max_y() < phone.composer.min_y());
}

#[test]
fn tiny_viewports_never_produce_negative_geometry() {
    let geometry = WorkspaceLayout::compute(260.0, 180.0, Insets::ZERO);
    for rect in [
        geometry.sidebar,
        geometry.top_bar,
        geometry.transcript,
        geometry.composer,
    ] {
        assert!(rect.width() >= 0.0);
        assert!(rect.height() >= 0.0);
    }
}
