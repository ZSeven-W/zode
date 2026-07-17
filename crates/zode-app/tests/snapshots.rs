mod snapshot_support;

use snapshot_support::{
    assert_platform_snapshot, fixture_state, GeometryExpectation, LayoutRect, SnapshotCase,
    SnapshotRoute,
};
use zode_app_model::ThemePreference;

const CONVERSATION_1221X992: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1221.0, 992.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 992.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 981.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 981.0, 992.0),
    GeometryExpectation::new(LayoutRect::Transcript, 362.5, 70.0, 736.0, 780.0),
    GeometryExpectation::new(LayoutRect::Composer, 362.5, 878.0, 736.0, 100.0),
];

const CONVERSATION_900X700: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 900.0, 700.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 64.0, 700.0),
    GeometryExpectation::new(LayoutRect::TopBar, 64.0, 0.0, 836.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 64.0, 0.0, 836.0, 700.0),
    GeometryExpectation::new(LayoutRect::Transcript, 114.0, 70.0, 736.0, 488.0),
    GeometryExpectation::new(LayoutRect::Composer, 114.0, 586.0, 736.0, 100.0),
];

const CONVERSATION_640X900: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 640.0, 900.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 0.0, 900.0),
    GeometryExpectation::new(LayoutRect::TopBar, 0.0, 0.0, 640.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 0.0, 0.0, 640.0, 900.0),
    GeometryExpectation::new(LayoutRect::Transcript, 16.0, 70.0, 608.0, 688.0),
    GeometryExpectation::new(LayoutRect::Composer, 16.0, 786.0, 608.0, 100.0),
];

const SETTINGS_1440X900: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1440.0, 900.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 900.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1200.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1200.0, 900.0),
    GeometryExpectation::new(LayoutRect::PageContent, 456.0, 70.0, 768.0, 830.0),
];

const INTEGRATIONS_1728X1117: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1728.0, 1117.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1117.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1488.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1488.0, 1117.0),
    GeometryExpectation::new(LayoutRect::PageContent, 616.0, 70.0, 736.0, 1047.0),
];

const ENVIRONMENT_1800X1080: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 1560.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 1560.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 652.0, 70.0, 736.0, 868.0),
    GeometryExpectation::new(LayoutRect::Composer, 652.0, 966.0, 736.0, 100.0),
    GeometryExpectation::new(LayoutRect::ContextPanel, 1484.0, 62.0, 300.0, 1002.0),
];

const REVIEW_1800X1080: &[GeometryExpectation] = &[
    GeometryExpectation::new(LayoutRect::Viewport, 0.0, 0.0, 1800.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Sidebar, 0.0, 0.0, 240.0, 1080.0),
    GeometryExpectation::new(LayoutRect::TopBar, 240.0, 0.0, 859.0, 46.0),
    GeometryExpectation::new(LayoutRect::PrimarySurface, 240.0, 0.0, 859.0, 1080.0),
    GeometryExpectation::new(LayoutRect::Transcript, 301.5, 70.0, 736.0, 868.0),
    GeometryExpectation::new(LayoutRect::Composer, 301.5, 966.0, 736.0, 100.0),
    GeometryExpectation::new(LayoutRect::Divider, 1099.0, 0.0, 1.0, 1080.0),
    GeometryExpectation::new(LayoutRect::ReviewPanel, 1100.0, 0.0, 700.0, 1080.0),
];

#[test]
fn reference_snapshots_match_platform_goldens() {
    let cases = [
        (
            SnapshotCase::new(
                "conversation-light-1221x992",
                1221,
                992,
                1.0,
                CONVERSATION_1221X992,
            ),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new(
                "conversation-dark-1221x992",
                1221,
                992,
                1.0,
                CONVERSATION_1221X992,
            ),
            SnapshotRoute::Conversation,
            ThemePreference::Dark,
        ),
        (
            SnapshotCase::new(
                "conversation-compact-900x700",
                900,
                700,
                1.0,
                CONVERSATION_900X700,
            ),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new(
                "conversation-phone-640x900",
                640,
                900,
                1.0,
                CONVERSATION_640X900,
            ),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new(
                "settings-1440x900-scale-1_25",
                1440,
                900,
                1.25,
                SETTINGS_1440X900,
            ),
            SnapshotRoute::Settings,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new(
                "integrations-1728x1117-scale-2",
                1728,
                1117,
                2.0,
                INTEGRATIONS_1728X1117,
            ),
            SnapshotRoute::Integrations,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new(
                "environment-1800x1080",
                1800,
                1080,
                1.0,
                ENVIRONMENT_1800X1080,
            ),
            SnapshotRoute::Environment,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("review-1800x1080", 1800, 1080, 1.0, REVIEW_1800X1080),
            SnapshotRoute::Review,
            ThemePreference::Light,
        ),
    ];

    for (case, route, theme) in cases {
        let state = fixture_state(route, theme, case.width);
        assert_platform_snapshot(case, &state);
    }
}
