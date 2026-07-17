use jian_widgets::Rect;
use zode_app_model::{IntegrationsTab, LayoutClass, SecondaryPane, SettingsCategory, ShellRoute};
use zode_app_ui::{
    Insets, PinnedSummaryMode, RectExt, WorkspaceLayout, PRIMARY_SIDEBAR_DEFAULT_W,
    SECONDARY_SIDEBAR_MAX_VIEWPORT_RATIO,
};

const EPSILON: f32 = 0.01;

#[test]
fn conversation_matches_the_wide_reference_contract() {
    let layout = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        None,
    );

    assert_eq!(layout.sidebar.width(), PRIMARY_SIDEBAR_DEFAULT_W);
    assert_eq!(layout.top_bar.height(), 46.0);
    assert_eq!(layout.transcript.width(), 736.0);
    assert_eq!(layout.composer.width(), 736.0);
    assert!((layout.transcript.min_x() - 678.5).abs() <= EPSILON);
}

#[test]
fn environment_panel_preserves_the_centered_conversation() {
    let conversation = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        None,
    );
    let environment = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Environment),
    );

    assert_eq!(environment.transcript, conversation.transcript);
    assert_eq!(environment.composer, conversation.composer);
    assert_eq!(environment.context_panel.width(), 300.0);
    assert_eq!(environment.context_panel.max_x(), 1800.0 - 16.0);
    assert_eq!(environment.review_panel.width(), 0.0);
    assert_eq!(environment.divider.width(), 0.0);
}

#[test]
fn environment_panel_never_overlaps_the_conversation_near_its_breakpoint() {
    for width in [1400.0, 1500.0, 1603.0] {
        let layout = WorkspaceLayout::compute_presentation(
            width,
            900.0,
            Insets::ZERO,
            ShellRoute::Conversation,
            Some(SecondaryPane::Environment),
        );

        assert_eq!(layout.context_panel.width(), 300.0);
        assert!(layout.transcript.max_x() <= layout.context_panel.min_x());
        assert!(layout.composer.max_x() <= layout.context_panel.min_x());
    }
}

#[test]
fn review_panel_creates_the_reference_split() {
    let layout = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Review),
    );

    let constrained_width = 1_800.0 * SECONDARY_SIDEBAR_MAX_VIEWPORT_RATIO;
    let panel_x = 1_800.0 - constrained_width;
    assert!((layout.review_panel.min_x() - panel_x).abs() <= EPSILON);
    assert!((layout.review_panel.width() - constrained_width).abs() <= EPSILON);
    assert!((layout.divider.min_x() - (panel_x - 1.0)).abs() <= EPSILON);
    assert_eq!(layout.divider.width(), 1.0);
    assert!((layout.transcript.min_x() - 336.5).abs() <= EPSILON);
    assert_eq!(layout.transcript.width(), 736.0);
    assert_eq!(layout.primary_surface.max_x(), layout.divider.min_x());
}

#[test]
fn full_page_routes_use_their_reference_content_widths() {
    let settings = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Settings(SettingsCategory::General),
        None,
    );
    let integrations = WorkspaceLayout::compute_presentation(
        1800.0,
        1080.0,
        Insets::ZERO,
        ShellRoute::Integrations(IntegrationsTab::Plugins),
        None,
    );

    assert_eq!(settings.page_content.width(), 768.0);
    assert!((settings.page_content.min_x() - 636.0).abs() <= EPSILON);
    assert_eq!(integrations.page_content.width(), 736.0);
    assert!((integrations.page_content.min_x() - 678.5).abs() <= EPSILON);
}

#[test]
fn presentation_breakpoints_float_the_summary_and_collapse_the_rail() {
    let medium = WorkspaceLayout::compute_presentation(
        1399.0,
        900.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Review),
    );
    let compact = WorkspaceLayout::compute_presentation(
        959.0,
        900.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Environment),
    );
    let phone = WorkspaceLayout::compute_presentation(
        719.0,
        900.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Environment),
    );

    assert_eq!(medium.class, LayoutClass::Wide);
    assert_eq!(medium.context_panel.width(), 0.0);
    assert_eq!(medium.review_panel.width(), 0.0);
    assert_eq!(medium.divider.width(), 0.0);
    assert_eq!(compact.class, LayoutClass::Compact);
    assert_eq!(compact.sidebar.width(), 64.0);
    assert_eq!(compact.context_panel.width(), 300.0);
    assert_eq!(compact.pinned_summary, PinnedSummaryMode::Overlay);
    assert!(compact.context_panel.min_x() > compact.composer.min_x());
    assert_eq!(phone.class, LayoutClass::Phone);
    assert_eq!(phone.sidebar.width(), 0.0);
    assert_eq!(phone.context_panel.width(), 300.0);
    assert_eq!(phone.pinned_summary, PinnedSummaryMode::Overlay);
}

#[test]
fn docked_summary_recenters_the_conversation_in_the_remaining_column() {
    let layout = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        ShellRoute::Conversation,
        Some(SecondaryPane::Environment),
    );

    assert_eq!(layout.pinned_summary, PinnedSummaryMode::Docked);
    assert_eq!(
        layout.context_panel,
        Rect::xywh(1_484.0, 62.0, 300.0, 1_002.0)
    );
    assert_eq!(layout.composer.origin.x, 494.0);
    assert_eq!(layout.transcript.origin.x, 494.0);
    assert_eq!(layout.composer.size.x, 736.0);
}

#[test]
fn every_presentation_rect_stays_finite_and_inside_extreme_viewports() {
    for (width, height, insets) in [
        (
            390.0,
            180.0,
            Insets {
                top: f32::INFINITY,
                right: f32::NAN,
                bottom: 400.0,
                left: 500.0,
            },
        ),
        (
            f32::INFINITY,
            f32::NAN,
            Insets {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            },
        ),
    ] {
        let layout = WorkspaceLayout::compute_presentation(
            width,
            height,
            insets,
            ShellRoute::Conversation,
            Some(SecondaryPane::Review),
        );
        let viewport_width = layout.viewport.width();
        let viewport_height = layout.viewport.height();

        for rect in [
            layout.viewport,
            layout.sidebar,
            layout.top_bar,
            layout.primary_surface,
            layout.transcript,
            layout.composer,
            layout.page_content,
            layout.context_panel,
            layout.divider,
            layout.review_panel,
        ] {
            assert!(rect.min_x().is_finite() && rect.min_y().is_finite());
            assert!(rect.width().is_finite() && rect.height().is_finite());
            assert!(rect.min_x() >= 0.0 && rect.min_y() >= 0.0);
            assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
            assert!(rect.max_x() <= viewport_width + EPSILON);
            assert!(rect.max_y() <= viewport_height + EPSILON);
        }
    }
}
