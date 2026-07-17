use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agent::message::{ContentBlock, Header, Message, MessageStore};
use zode_app_runtime::{path_to_workspace_uri, workspace_uri_to_path, LocalSessionRepository};
use zode_core::session_store::{SessionSaveOutcome, SessionWriteMode};
use zode_node_protocol::{EndpointErrorKind, NodeId, SessionLocator, ThreadStatus, WorkspaceUri};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zode-app-session-repository-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn locator(node_id: NodeId, session_id: &str) -> SessionLocator {
    SessionLocator::new(node_id, session_id)
}

fn workspace(path: &Path) -> WorkspaceUri {
    path_to_workspace_uri(path).unwrap()
}

fn store_with(text: &str) -> MessageStore {
    let mut store = MessageStore::new();
    store
        .push(Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        })
        .unwrap();
    store
}

#[tokio::test]
async fn foreign_node_session_is_rejected_without_writing() {
    let dir = TestDir::new("foreign-node");
    let local_node = NodeId::new();
    let repository = LocalSessionRepository::new(dir.path(), local_node);
    let foreign = locator(NodeId::new(), "foreign-session");
    let workspace = workspace(&dir.path().join("project"));

    let result = repository
        .create(&foreign, &workspace, "test-model".to_string())
        .await;

    let error = result.expect_err("a local repository must reject a foreign node");
    assert_eq!(error.kind, EndpointErrorKind::CapabilityDenied);
    assert!(!dir.path().join("sessions").exists());
}

#[tokio::test]
async fn create_adopts_caller_session_id_and_persists_metadata() {
    let dir = TestDir::new("create");
    let node_id = NodeId::new();
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let session = locator(node_id, "caller-allocated-id");
    let project = dir.path().join("project with spaces").join("中文");
    let workspace = workspace(&project);

    let loaded = repository
        .create(&session, &workspace, "test-model".to_string())
        .await
        .unwrap();

    assert_eq!(loaded.meta.id, "caller-allocated-id");
    assert_eq!(loaded.meta.cwd, project.to_string_lossy());
    assert_eq!(loaded.meta.model, "test-model");
    assert!(loaded.store.is_empty());
    assert!(dir
        .path()
        .join("sessions/caller-allocated-id.jsonl")
        .is_file());
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("sessions/index.json")).unwrap()).unwrap();
    assert_eq!(index["sessions"][0]["id"], "caller-allocated-id");
}

#[tokio::test]
async fn duplicate_create_is_an_invalid_request_and_preserves_original() {
    let dir = TestDir::new("duplicate");
    let node_id = NodeId::new();
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let session = locator(node_id, "duplicate-id");
    let first_workspace = workspace(&dir.path().join("first"));
    let second_workspace = workspace(&dir.path().join("second"));
    repository
        .create(&session, &first_workspace, "first-model".to_string())
        .await
        .unwrap();

    let result = repository
        .create(&session, &second_workspace, "second-model".to_string())
        .await;

    let error = result.expect_err("duplicate caller identities must be rejected");
    assert_eq!(error.kind, EndpointErrorKind::InvalidRequest);
    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.cwd, dir.path().join("first").to_string_lossy());
    assert_eq!(loaded.meta.model, "first-model");
}

#[tokio::test]
async fn list_and_load_roundtrip_after_repository_restart() {
    let dir = TestDir::new("restart");
    let node_id = NodeId::new();
    let session = locator(node_id, "restart-id");
    let project = dir.path().join("restart project");
    let workspace = workspace(&project);
    let first = LocalSessionRepository::new(dir.path(), node_id);
    let loaded = first
        .create(&session, &workspace, "restart-model".to_string())
        .await
        .unwrap();
    let outcome = first
        .save(
            &session,
            loaded.meta,
            store_with("persist me"),
            SessionWriteMode::Full,
        )
        .await
        .unwrap();
    assert!(matches!(outcome, SessionSaveOutcome::Saved { .. }));
    drop(first);

    let restarted = LocalSessionRepository::new(dir.path(), node_id);
    let threads = restarted.list().unwrap();
    let loaded = restarted.load(&session).await.unwrap();

    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].session, session);
    assert_eq!(threads[0].workspace_uri, workspace);
    assert_eq!(threads[0].status, ThreadStatus::Idle);
    assert_eq!(loaded.meta.id, "restart-id");
    assert_eq!(loaded.meta.model, "restart-model");
    assert_eq!(loaded.store.len(), 1);
}

#[test]
fn local_workspace_uri_roundtrips_spaces_and_non_ascii_without_whitespace() {
    let dir = TestDir::new("workspace-uri");
    let path = dir.path().join("project with spaces").join("设计");

    let uri = path_to_workspace_uri(&path).unwrap();
    let decoded = workspace_uri_to_path(&uri).unwrap();

    assert!(uri.as_str().starts_with("file:///"));
    assert!(uri.as_str().contains("%20"));
    assert!(!uri.as_str().chars().any(char::is_whitespace));
    assert_eq!(decoded, path);
}

#[tokio::test]
async fn explicit_rename_and_model_update_survive_a_later_stale_transcript_save() {
    let dir = TestDir::new("metadata-update");
    let node_id = NodeId::new();
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let session = locator(node_id, "metadata-id");
    let workspace = workspace(&dir.path().join("project"));
    let stale = repository
        .create(&session, &workspace, "old-model".to_string())
        .await
        .unwrap();
    repository
        .rename(&session, "renamed session".to_string())
        .await
        .unwrap();
    repository
        .update_model(&session, "new-model".to_string())
        .await
        .unwrap();

    let outcome = repository
        .save(
            &session,
            stale.meta,
            store_with("new transcript"),
            SessionWriteMode::Full,
        )
        .await
        .unwrap();
    assert!(matches!(outcome, SessionSaveOutcome::Saved { .. }));

    let loaded = repository.load(&session).await.unwrap();
    assert_eq!(loaded.meta.title, "renamed session");
    assert_eq!(loaded.meta.model, "new-model");
    assert_eq!(loaded.store.len(), 1);
}

#[tokio::test]
async fn delete_removes_transcript_and_index_entry() {
    let dir = TestDir::new("delete");
    let node_id = NodeId::new();
    let repository = LocalSessionRepository::new(dir.path(), node_id);
    let session = locator(node_id, "delete-id");
    let workspace = workspace(&dir.path().join("project"));
    repository
        .create(&session, &workspace, "model".to_string())
        .await
        .unwrap();

    repository.delete(&session).await.unwrap();

    assert!(repository.list().unwrap().is_empty());
    assert!(!dir.path().join("sessions/delete-id.jsonl").exists());
    let error = repository
        .load(&session)
        .await
        .expect_err("a deleted session must not load");
    assert_eq!(error.kind, EndpointErrorKind::NotFound);
}

#[test]
fn corrupt_index_returns_stable_error_without_overwriting_original_bytes() {
    let dir = TestDir::new("corrupt-index");
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let index = sessions.join("index.json");
    let corrupt = b"{not valid json";
    fs::write(&index, corrupt).unwrap();
    let repository = LocalSessionRepository::new(dir.path(), NodeId::new());

    let error = repository
        .list()
        .expect_err("corrupt metadata must not become an empty index");

    assert_eq!(error.kind, EndpointErrorKind::Internal);
    assert_eq!(fs::read(index).unwrap(), corrupt);
}
