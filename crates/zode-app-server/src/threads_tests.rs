use crate::threads::ThreadRegistry;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_app_server_protocol::types::{ThreadStartParams, ThreadStatus};

#[test]
fn start_thread_records_metadata() {
    let mut registry = ThreadRegistry::default();
    let thread = registry
        .start_metadata_only(
            ThreadStartParams {
                cwd: Some("/tmp/project".to_string()),
                model: Some("model-a".to_string()),
            },
            "generated-title".to_string(),
        )
        .unwrap();
    assert_eq!(thread.name, "generated-title");
    assert_eq!(thread.cwd, "/tmp/project");
    assert_eq!(thread.model, "model-a");
    assert_eq!(thread.status, ThreadStatus::Loaded);
    assert_eq!(registry.list().len(), 1);
}

#[test]
fn name_set_updates_existing_thread() {
    let mut registry = ThreadRegistry::default();
    let thread = registry
        .start_metadata_only(
            ThreadStartParams {
                cwd: Some("/tmp/project".to_string()),
                model: Some("model-a".to_string()),
            },
            "old".to_string(),
        )
        .unwrap();
    registry.set_name(&thread.id, "new").unwrap();
    assert_eq!(registry.read(&thread.id).unwrap().name, "new");
}

#[test]
fn delete_removes_existing_thread() {
    let mut registry = ThreadRegistry::default();
    let thread = registry
        .start_metadata_only(
            ThreadStartParams {
                cwd: None,
                model: None,
            },
            "delete me".to_string(),
        )
        .unwrap();

    registry.delete(&thread.id).unwrap();

    assert!(registry.read(&thread.id).is_err());
    assert!(registry.list().is_empty());
}

#[test]
fn missing_thread_operations_return_invalid_params() {
    let mut registry = ThreadRegistry::default();

    for err in [
        registry.read("missing").unwrap_err(),
        registry.set_name("missing", "new").unwrap_err(),
        registry.delete("missing").unwrap_err(),
    ] {
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("thread not found"));
    }
}
