mod snapshot_support;

use snapshot_support::{assert_platform_snapshot, fixture_state, SnapshotCase, SnapshotRoute};
use zode_app_model::ThemePreference;

#[test]
fn reference_snapshots_match_platform_goldens() {
    let cases = [
        (
            SnapshotCase::new("conversation-light-1221x992", 1221, 992, 1.0),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("conversation-dark-1221x992", 1221, 992, 1.0),
            SnapshotRoute::Conversation,
            ThemePreference::Dark,
        ),
        (
            SnapshotCase::new("conversation-compact-900x700", 900, 700, 1.0),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("conversation-phone-640x900", 640, 900, 1.0),
            SnapshotRoute::Conversation,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("settings-1440x900-scale-1_25", 1440, 900, 1.25),
            SnapshotRoute::Settings,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("integrations-1728x1117-scale-2", 1728, 1117, 2.0),
            SnapshotRoute::Integrations,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("environment-1800x1080", 1800, 1080, 1.0),
            SnapshotRoute::Environment,
            ThemePreference::Light,
        ),
        (
            SnapshotCase::new("review-1800x1080", 1800, 1080, 1.0),
            SnapshotRoute::Review,
            ThemePreference::Light,
        ),
    ];

    for (case, route, theme) in cases {
        let state = fixture_state(route, theme, case.width);
        assert_platform_snapshot(case, &state);
    }
}
