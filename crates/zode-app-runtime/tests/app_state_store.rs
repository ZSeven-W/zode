use zode_app_model::{ThemePreference, UiPreferences};
use zode_app_runtime::{AppStateFile, AppStateStore, SessionUiState, TaskContext, WindowGeometry};
use zode_node_protocol::WorkspaceUri;

#[test]
fn reconcile_drops_ui_metadata_for_deleted_sessions() {
    let mut app = AppStateFile::default();
    app.sessions.insert(
        "alive".into(),
        SessionUiState {
            pinned: true,
            archived: false,
            unread: false,
            failed: false,
        },
    );
    app.sessions.insert(
        "deleted".into(),
        SessionUiState {
            pinned: false,
            archived: false,
            unread: true,
            failed: true,
        },
    );
    app.last_session = Some("deleted".into());

    app.reconcile(["alive"]);

    assert!(app.sessions.contains_key("alive"));
    assert!(!app.sessions.contains_key("deleted"));
    assert_eq!(app.last_session, None);
}

#[test]
fn store_round_trips_versioned_state_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    let mut state = AppStateFile {
        last_session: Some("session-1".into()),
        ..AppStateFile::default()
    };
    state.sessions.insert(
        "session-1".into(),
        SessionUiState {
            pinned: true,
            archived: true,
            unread: true,
            failed: false,
        },
    );
    state
        .collapsed_workspaces
        .insert("file:///repo/zode".into());
    state.ui_preferences = UiPreferences {
        theme: ThemePreference::Dark,
        reduced_motion: true,
        high_contrast: true,
        task_suggestions: false,
        sidebar_tasks_expanded: false,
    };
    state.window_geometry = Some(WindowGeometry {
        x: -1800,
        y: 60,
        width: 1200,
        height: 900,
        maximized: true,
    });

    store.save(&state).unwrap();
    let loaded = store.load().unwrap();
    let json = std::fs::read_to_string(store.path()).unwrap();

    assert_eq!(loaded, state);
    assert_eq!(store.path(), directory.path().join("app-state.json"));
    assert!(!json.contains("systemTheme"));
}

#[test]
fn missing_state_file_loads_default_version() {
    let directory = tempfile::tempdir().unwrap();
    let loaded = AppStateStore::new(directory.path()).load().unwrap();

    assert_eq!(loaded.version, 1);
    assert!(loaded.sessions.is_empty());
    assert_eq!(loaded.ui_preferences, UiPreferences::default());
    assert_eq!(loaded.window_geometry, None);
    assert_eq!(loaded.task_context, None);
}

#[test]
fn legacy_v1_missing_appearance_and_window_fields_uses_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    std::fs::write(
        store.path(),
        r#"{
            "version": 1,
            "lastSession": "legacy-session",
            "sessions": {
                "legacy-session": {
                    "pinned": true,
                    "unread": false,
                    "failed": false
                }
            },
            "collapsedWorkspaces": []
        }"#,
    )
    .unwrap();

    let loaded = store.load().unwrap();

    assert_eq!(loaded.last_session.as_deref(), Some("legacy-session"));
    assert!(!loaded.sessions["legacy-session"].archived);
    assert_eq!(loaded.ui_preferences, UiPreferences::default());
    assert_eq!(loaded.window_geometry, None);
    assert_eq!(loaded.task_context, None);
}

#[test]
fn task_context_round_trips_unset_project_and_projectless_states() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    let project = WorkspaceUri::new("file:///repo/zode").unwrap();
    let contexts = [
        None,
        Some(TaskContext::Project {
            workspace_uri: project.clone(),
        }),
        Some(TaskContext::Projectless),
    ];

    for expected in contexts {
        let state = AppStateFile {
            task_context: expected.clone(),
            ..AppStateFile::default()
        };
        store.save(&state).unwrap();
        assert_eq!(store.load().unwrap().task_context, expected);
    }

    let project_json = serde_json::to_value(AppStateFile {
        task_context: Some(TaskContext::Project {
            workspace_uri: project,
        }),
        ..AppStateFile::default()
    })
    .unwrap();
    assert_eq!(project_json["taskContext"]["kind"], "project");
    assert_eq!(
        project_json["taskContext"]["workspaceUri"],
        "file:///repo/zode"
    );
}

#[test]
fn partial_legacy_preferences_use_field_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    std::fs::write(
        store.path(),
        r#"{
            "version": 1,
            "uiPreferences": { "theme": "dark" }
        }"#,
    )
    .unwrap();

    let loaded = store.load().unwrap();

    assert_eq!(loaded.ui_preferences.theme, ThemePreference::Dark);
    assert!(!loaded.ui_preferences.reduced_motion);
    assert!(!loaded.ui_preferences.high_contrast);
    assert!(loaded.ui_preferences.task_suggestions);
    assert!(loaded.ui_preferences.sidebar_tasks_expanded);
}

#[test]
fn corrupt_state_is_reported_without_overwriting_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    let corrupt = b"{ this is not valid JSON";
    std::fs::write(store.path(), corrupt).unwrap();

    assert!(store.load().is_err());
    assert_eq!(std::fs::read(store.path()).unwrap(), corrupt);
}

#[test]
fn object_with_no_fields_loads_the_v1_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    std::fs::write(store.path(), "{}").unwrap();

    assert_eq!(store.load().unwrap(), AppStateFile::default());
}

#[test]
fn update_preserves_unrelated_state_fields() {
    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());
    let mut original = AppStateFile {
        last_session: Some("session-1".into()),
        ..AppStateFile::default()
    };
    original.sessions.insert(
        "session-1".into(),
        SessionUiState {
            pinned: true,
            archived: true,
            unread: false,
            failed: true,
        },
    );
    original
        .collapsed_workspaces
        .insert("file:///repo/zode".into());
    original.task_context = Some(TaskContext::Projectless);
    store.save(&original).unwrap();

    store
        .update(|state| {
            state.ui_preferences.theme = ThemePreference::Light;
            state.ui_preferences.reduced_motion = true;
            state.window_geometry = Some(WindowGeometry {
                x: 100,
                y: 80,
                width: 1221,
                height: 992,
                maximized: false,
            });
        })
        .unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.last_session, original.last_session);
    assert_eq!(loaded.sessions, original.sessions);
    assert_eq!(loaded.collapsed_workspaces, original.collapsed_workspaces);
    assert_eq!(loaded.task_context, original.task_context);
    assert_eq!(loaded.ui_preferences.theme, ThemePreference::Light);
    assert!(loaded.ui_preferences.reduced_motion);
    assert_eq!(loaded.window_geometry.unwrap().width, 1221);
}

#[test]
fn update_holds_the_advisory_lock_while_mutating() {
    use zode_core::{persistence::AdvisoryFileLock, CoreError};

    let directory = tempfile::tempdir().unwrap();
    let store = AppStateStore::new(directory.path());

    store
        .update(|_| {
            assert!(matches!(
                AdvisoryFileLock::try_acquire(store.path()),
                Err(CoreError::Busy(_)),
            ));
        })
        .unwrap();
}

#[test]
fn concurrent_updates_do_not_lose_independent_changes() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    use std::time::{Duration, Instant};

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(AppStateStore::new(directory.path()));
    store.save(&AppStateFile::default()).unwrap();
    let start = Arc::new(Barrier::new(3));
    let closure_entries = Arc::new(AtomicUsize::new(0));

    let spawn_update = |update: fn(&mut AppStateFile)| {
        let store = store.clone();
        let start = start.clone();
        let closure_entries = closure_entries.clone();
        std::thread::spawn(move || {
            start.wait();
            store
                .update(|state| {
                    closure_entries.fetch_add(1, Ordering::SeqCst);
                    let deadline = Instant::now() + Duration::from_millis(500);
                    while closure_entries.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                        std::thread::yield_now();
                    }
                    update(state);
                })
                .unwrap();
        })
    };

    let update_session = spawn_update(|state| state.last_session = Some("session-1".into()));
    let update_workspace = spawn_update(|state| {
        state
            .collapsed_workspaces
            .insert("file:///repo/zode".into());
    });
    start.wait();
    update_session.join().unwrap();
    update_workspace.join().unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.last_session.as_deref(), Some("session-1"));
    assert!(loaded.collapsed_workspaces.contains("file:///repo/zode"));
}

#[test]
fn concurrent_reconcile_does_not_erase_preferences_or_window_geometry() {
    use std::sync::{Arc, Barrier};

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(AppStateStore::new(directory.path()));
    let mut initial = AppStateFile::default();
    initial.sessions.insert(
        "alive".into(),
        SessionUiState {
            pinned: true,
            archived: false,
            unread: false,
            failed: false,
        },
    );
    initial.sessions.insert(
        "deleted".into(),
        SessionUiState {
            pinned: false,
            archived: false,
            unread: true,
            failed: false,
        },
    );
    store.save(&initial).unwrap();
    let start = Arc::new(Barrier::new(3));

    let reconcile = {
        let store = store.clone();
        let start = start.clone();
        std::thread::spawn(move || {
            start.wait();
            store.update(|state| state.reconcile(["alive"])).unwrap();
        })
    };
    let update_ui = {
        let store = store.clone();
        let start = start.clone();
        std::thread::spawn(move || {
            start.wait();
            store
                .update(|state| {
                    state.ui_preferences.theme = ThemePreference::Dark;
                    state.window_geometry = Some(WindowGeometry {
                        x: -900,
                        y: 40,
                        width: 1221,
                        height: 992,
                        maximized: true,
                    });
                })
                .unwrap();
        })
    };
    start.wait();
    reconcile.join().unwrap();
    update_ui.join().unwrap();

    let loaded = store.load().unwrap();
    assert_eq!(loaded.sessions.len(), 1);
    assert!(loaded.sessions.contains_key("alive"));
    assert_eq!(loaded.ui_preferences.theme, ThemePreference::Dark);
    assert_eq!(loaded.window_geometry.unwrap().x, -900);
}
