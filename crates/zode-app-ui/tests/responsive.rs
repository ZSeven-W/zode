use zode_app_model::{demo_state, LayoutClass, SettingsCategory, ShellRoute};
use zode_app_ui::{
    constrained_primary_sidebar_width, Insets, PinnedSummaryMode, PrimarySidebarLayoutOptions,
    RectExt, SecondarySidebarLayoutOptions, SidebarLayoutOptions, WorkspaceLayout,
    WorkspaceLayoutOptions, WorkspaceSnapshot, COMPOSER_BOTTOM, COMPOSER_H, CONTENT_W,
    PRIMARY_SIDEBAR_DEFAULT_W, SIDEBAR_W,
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
    assert_eq!(geometry.sidebar.width(), PRIMARY_SIDEBAR_DEFAULT_W);
    assert_eq!(geometry.transcript.width(), CONTENT_W);
    assert_eq!(geometry.composer.width(), CONTENT_W);
    assert_eq!(geometry.composer.max_y(), 992.0 - COMPOSER_BOTTOM);
}

#[test]
fn optional_context_panel_preserves_width_and_recenters_the_wide_conversation() {
    let without_panel = WorkspaceLayout::compute(1800.0, 1080.0, Insets::ZERO);
    let with_panel = WorkspaceLayout::compute_with_options(
        1800.0,
        1080.0,
        Insets::ZERO,
        WorkspaceLayoutOptions {
            context_panel_width: 300.0,
            ..WorkspaceLayoutOptions::default()
        },
    );

    assert_eq!(without_panel.sidebar.width(), PRIMARY_SIDEBAR_DEFAULT_W);
    assert_eq!(without_panel.transcript.width(), 736.0);
    assert_eq!(
        with_panel.transcript.width(),
        without_panel.transcript.width()
    );
    assert_eq!(with_panel.composer.width(), without_panel.composer.width());
    assert_eq!(with_panel.context_panel.width(), 300.0);
    let remaining_width = with_panel.context_panel.min_x() - with_panel.primary_surface.min_x();
    let expected_x = with_panel.primary_surface.min_x()
        + (remaining_width - with_panel.transcript.width()) / 2.0;
    assert_eq!(with_panel.transcript.min_x(), expected_x);
    assert_eq!(with_panel.composer.min_x(), expected_x);
    assert!(with_panel.context_panel.min_x() >= with_panel.composer.max_x());
}

#[test]
fn primary_sidebar_supports_the_reference_width_and_viewport_clamps() {
    assert_eq!(constrained_primary_sidebar_width(1_800.0, 100.0), 220.0);
    assert_eq!(constrained_primary_sidebar_width(1_800.0, 293.0), 293.0);
    assert_eq!(constrained_primary_sidebar_width(1_800.0, 2_000.0), 540.0);

    let geometry = WorkspaceLayout::compute_with_options(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        WorkspaceLayoutOptions {
            primary_sidebar: PrimarySidebarLayoutOptions {
                open: true,
                width: 293.0,
            },
            ..WorkspaceLayoutOptions::default()
        },
    );
    assert_eq!(geometry.sidebar.width(), 293.0);
    assert_eq!(geometry.primary_surface.min_x(), 293.0);
}

#[test]
fn primary_sidebar_can_hide_without_changing_settings_navigation() {
    let hidden = WorkspaceLayout::compute_with_options(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        WorkspaceLayoutOptions {
            primary_sidebar: PrimarySidebarLayoutOptions {
                open: false,
                width: 293.0,
            },
            ..WorkspaceLayoutOptions::default()
        },
    );
    assert_eq!(hidden.sidebar.width(), 0.0);
    assert_eq!(hidden.primary_surface.min_x(), 0.0);
    assert_eq!(hidden.primary_sidebar_divider.height(), 0.0);

    let settings = WorkspaceLayout::compute_presentation_with_sidebar_options(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        ShellRoute::Settings(SettingsCategory::General),
        None,
        COMPOSER_H,
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: false,
                width: 293.0,
            },
            secondary: SecondarySidebarLayoutOptions::default(),
        },
    );
    assert_eq!(settings.sidebar.width(), SIDEBAR_W);
    assert_eq!(settings.primary_sidebar_divider.height(), 0.0);
}

#[test]
fn workspace_snapshot_uses_persisted_primary_sidebar_state() {
    let mut state = demo_state();
    state.ui_preferences.primary_sidebar_width = 293;
    let resized = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert_eq!(resized.layout.sidebar.width(), 293.0);

    state.shell.sidebar_open = false;
    let hidden = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert_eq!(hidden.layout.sidebar.width(), 0.0);
}

#[test]
fn resized_primary_preserves_the_open_secondary_panel_and_main_minimum() {
    let baseline = WorkspaceLayout::compute_presentation_with_sidebar_options(
        1_400.0,
        1_000.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        None,
        COMPOSER_H,
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions::default(),
            secondary: SecondarySidebarLayoutOptions {
                open: true,
                width: 700.0,
                pinned_summary_overlay: false,
            },
        },
    );
    let resized = WorkspaceLayout::compute_presentation_with_sidebar_options(
        1_400.0,
        1_000.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        None,
        COMPOSER_H,
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: true,
                width: 420.0,
            },
            secondary: SecondarySidebarLayoutOptions {
                open: true,
                width: 700.0,
                pinned_summary_overlay: false,
            },
        },
    );

    assert!(baseline.review_panel.width() > 0.0);
    assert!(resized.review_panel.width() > 0.0);
    assert!(resized.primary_surface.width() >= 560.0);
    assert_eq!(resized.review_panel.max_x(), baseline.review_panel.max_x());
}

#[test]
fn resized_primary_preserves_the_docked_pinned_summary() {
    let resized = WorkspaceLayout::compute_presentation_with_sidebar_options(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(zode_app_model::SecondaryPane::Environment),
        COMPOSER_H,
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: true,
                width: 293.0,
            },
            secondary: SecondarySidebarLayoutOptions::default(),
        },
    );

    assert_eq!(resized.sidebar.width(), 293.0);
    assert!(resized.primary_surface.width() >= 560.0);
    assert_eq!(resized.pinned_summary, PinnedSummaryMode::Docked);
    assert_eq!(resized.context_panel.width(), 300.0);
}

#[test]
fn narrow_window_hides_secondary_split_and_overlays_the_pinned_summary() {
    let narrow = WorkspaceLayout::compute_presentation_with_sidebar_options(
        1_200.0,
        900.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(zode_app_model::SecondaryPane::Environment),
        COMPOSER_H,
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: true,
                width: 293.0,
            },
            secondary: SecondarySidebarLayoutOptions {
                open: true,
                width: 700.0,
                pinned_summary_overlay: false,
            },
        },
    );

    assert_eq!(narrow.sidebar.width(), 293.0);
    assert_eq!(narrow.review_panel.width(), 0.0);
    assert_eq!(narrow.divider.width(), 0.0);
    assert_eq!(narrow.pinned_summary, PinnedSummaryMode::Overlay);
    assert_eq!(narrow.context_panel.width(), 300.0);
}
