use zode_app_runtime::{persist_project_allow, PersistedApproval};
use zode_core::{persist_allow_always, revoke_allow_always};

#[test]
fn allow_always_persists_and_deduplicates_project_state() {
    let project = tempfile::tempdir().unwrap();
    persist_allow_always(project.path(), "Bash").unwrap();
    persist_allow_always(project.path(), "Bash").unwrap();
    persist_allow_always(project.path(), "FileEdit").unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.path().join(".zode/state.json")).unwrap())
            .unwrap();
    assert_eq!(
        value["permissions"]["allow"],
        serde_json::json!(["Bash", "FileEdit"]),
    );
}

#[test]
fn revoke_removes_only_the_selected_project_permission() {
    let project = tempfile::tempdir().unwrap();
    persist_allow_always(project.path(), "Bash").unwrap();
    persist_allow_always(project.path(), "FileEdit").unwrap();
    revoke_allow_always(project.path(), "Bash").unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.path().join(".zode/state.json")).unwrap())
            .unwrap();
    assert_eq!(
        value["permissions"]["allow"],
        serde_json::json!(["FileEdit"]),
    );
}

#[test]
fn persistence_failure_explicitly_downgrades_to_allow_once() {
    let root = tempfile::tempdir().unwrap();
    let invalid_project = root.path().join("not-a-directory");
    std::fs::write(&invalid_project, b"file").unwrap();

    let result = persist_project_allow(&invalid_project, "Bash");

    assert!(matches!(
        result,
        PersistedApproval::AllowOnceFallback { ref message }
            if message.contains("could not be persisted")
    ));
}
