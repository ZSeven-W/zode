use zode_app_runtime::{AppStateFile, AppStateStore, SessionUiState};

#[test]
fn reconcile_drops_ui_metadata_for_deleted_sessions() {
    let mut app = AppStateFile::default();
    app.sessions.insert(
        "alive".into(),
        SessionUiState {
            pinned: true,
            unread: false,
            failed: false,
        },
    );
    app.sessions.insert(
        "deleted".into(),
        SessionUiState {
            pinned: false,
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
            unread: true,
            failed: false,
        },
    );
    state
        .collapsed_workspaces
        .insert("file:///repo/zode".into());

    store.save(&state).unwrap();
    let loaded = store.load().unwrap();

    assert_eq!(loaded, state);
    assert_eq!(store.path(), directory.path().join("app-state.json"));
}

#[test]
fn missing_state_file_loads_default_version() {
    let directory = tempfile::tempdir().unwrap();
    let loaded = AppStateStore::new(directory.path()).load().unwrap();

    assert_eq!(loaded.version, 1);
    assert!(loaded.sessions.is_empty());
}
