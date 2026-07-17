use super::*;
use agent::message::{ContentBlock, Header, Message};
use agent::session::{SessionHeader, SCHEMA_VERSION};

fn user_message(text: &str) -> Message {
    Message::User {
        header: Header::new(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }
}

fn store_with(messages: &[&str]) -> MessageStore {
    let mut store = MessageStore::new();
    for text in messages {
        store.push(user_message(text)).unwrap();
    }
    store
}

fn meta(id: &str, title: &str, updated_at: u64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        title: title.to_string(),
        cwd: "/work/project".to_string(),
        model: "test-model".to_string(),
        updated_at,
    }
}

fn save(id: &str, title: &str, messages: &[&str]) -> SessionSave {
    save_with_generation(id, title, messages, 1)
}

fn save_with_generation(id: &str, title: &str, messages: &[&str], generation: u64) -> SessionSave {
    SessionSave {
        meta: meta(id, title, generation),
        store: store_with(messages),
        write_mode: SessionWriteMode::Full,
        generation,
    }
}

fn assert_saved(outcome: SessionSaveOutcome, expected: usize) {
    assert_eq!(
        outcome,
        SessionSaveOutcome::Saved {
            persisted_messages: expected
        }
    );
}

#[tokio::test]
async fn save_list_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    let original = store_with(&["hello", "again"]);

    let persisted = repository
        .save(SessionSave {
            meta: meta("roundtrip-1", "roundtrip", 42),
            store: original.clone(),
            write_mode: SessionWriteMode::Full,
            generation: 1,
        })
        .await
        .unwrap();

    assert_saved(persisted, 2);
    let listed = repository.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "roundtrip-1");
    let loaded = repository.load("roundtrip-1").await.unwrap();
    assert_eq!(
        loaded.iter().collect::<Vec<_>>(),
        original.iter().collect::<Vec<_>>()
    );
}

#[tokio::test]
#[serial_test::serial]
async fn explicit_config_dir_ignores_environment_and_session_cwd() {
    let explicit = tempfile::tempdir().unwrap();
    let ignored_env = tempfile::tempdir().unwrap();
    let ignored_cwd = tempfile::tempdir().unwrap();
    let previous = std::env::var_os("ZODE_CONFIG_DIR");
    std::env::set_var("ZODE_CONFIG_DIR", ignored_env.path());
    let repository = SessionRepository::new(explicit.path().to_path_buf());
    let mut session = save("explicit-1", "explicit", &["message"]);
    session.meta.cwd = ignored_cwd.path().display().to_string();

    assert_saved(repository.save(session).await.unwrap(), 1);

    if let Some(previous) = previous {
        std::env::set_var("ZODE_CONFIG_DIR", previous);
    } else {
        std::env::remove_var("ZODE_CONFIG_DIR");
    }
    assert!(explicit.path().join("sessions/explicit-1.jsonl").is_file());
    assert!(!ignored_env.path().join("sessions").exists());
    assert!(!ignored_cwd.path().join("sessions").exists());
}

#[tokio::test]
async fn rename_and_delete_share_the_repository_index() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save("rename-delete", "before", &["hello"]))
            .await
            .unwrap(),
        1,
    );

    repository
        .rename("rename-delete", "after".to_string())
        .await
        .unwrap();
    assert_eq!(repository.list().unwrap()[0].title, "after");

    repository.delete("rename-delete").await.unwrap();
    assert!(repository.list().unwrap().is_empty());
    assert!(!dir.path().join("sessions/rename-delete.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_saves_do_not_lose_index_entries() {
    let dir = tempfile::tempdir().unwrap();
    let left = SessionRepository::new(dir.path().to_path_buf());
    let right = left.clone();

    let (left_result, right_result) = tokio::join!(
        left.save(save("concurrent-a", "a", &["a"])),
        right.save(save("concurrent-b", "b", &["b"])),
    );
    assert_saved(left_result.unwrap(), 1);
    assert_saved(right_result.unwrap(), 1);

    let mut ids: Vec<_> = left
        .list()
        .unwrap()
        .into_iter()
        .map(|meta| meta.id)
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["concurrent-a", "concurrent-b"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_save_and_delete_do_not_lose_unrelated_update() {
    let dir = tempfile::tempdir().unwrap();
    let saver = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        saver
            .save(save("delete-me", "old", &["old"]))
            .await
            .unwrap(),
        1,
    );
    let deleter = saver.clone();

    let (save_result, delete_result) = tokio::join!(
        saver.save(save("keep-me", "new", &["new"])),
        deleter.delete("delete-me"),
    );
    assert_saved(save_result.unwrap(), 1);
    delete_result.unwrap();

    let listed = saver.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "keep-me");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn concurrent_rename_save_and_delete_preserve_unrelated_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    for request in [
        save("rename-me", "old title", &["rename"]),
        save("delete-me", "delete title", &["delete"]),
        save("keep-meta", "stable title", &["keep"]),
    ] {
        assert_saved(repository.save(request).await.unwrap(), 1);
    }

    let saver = repository.clone();
    let renamer = repository.clone();
    let deleter = repository.clone();
    let (save_result, rename_result, delete_result) = tokio::join!(
        saver.save(save("new-session", "new title", &["new"])),
        renamer.rename("rename-me", "renamed".to_string()),
        deleter.delete("delete-me"),
    );
    assert_saved(save_result.unwrap(), 1);
    rename_result.unwrap();
    delete_result.unwrap();

    let listed = repository.list().unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|meta| meta.id == "rename-me")
            .unwrap()
            .title,
        "renamed"
    );
    assert!(!listed.iter().any(|meta| meta.id == "delete-me"));
    assert!(listed.iter().any(|meta| meta.id == "new-session"));
    let stable = listed.iter().find(|meta| meta.id == "keep-meta").unwrap();
    assert_eq!(stable.title, "stable title");
    assert_eq!(stable.cwd, "/work/project");
    assert_eq!(stable.model, "test-model");
    assert_eq!(stable.updated_at, 1);
}

#[tokio::test]
async fn legacy_v1_jsonl_session_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    let header = SessionHeader {
        schema_version: SCHEMA_VERSION.to_string(),
        agent_version: "0.1.0".to_string(),
    };
    let message = user_message("legacy");
    let fixture = format!(
        "{}\n{}\n",
        serde_json::to_string(&header).unwrap(),
        serde_json::to_string(&message).unwrap()
    );
    std::fs::write(sessions_dir.join("legacy-1.jsonl"), fixture).unwrap();
    std::fs::write(
        sessions_dir.join("index.json"),
        r#"{"sessions":[{"id":"legacy-1","title":"legacy","cwd":"/tmp","model":"old-model","updated_at":1}]}"#,
    )
    .unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());

    let loaded = repository.load("legacy-1").await.unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.iter().next(), Some(&message));
    assert_eq!(repository.list().unwrap()[0].title, "legacy");
}

#[tokio::test]
async fn invalid_ids_are_rejected_and_legacy_safe_ids_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());

    for id in ["", ".", "..", "../escape", "a/b", "a\\b", "has space"] {
        assert!(matches!(
            repository.load(id).await,
            Err(CoreError::Other(message)) if message.contains("invalid session id")
        ));
    }
    assert_saved(
        repository
            .save(save("AZaz09-_", "valid", &[]))
            .await
            .unwrap(),
        0,
    );
    assert_eq!(repository.list().unwrap()[0].id, "AZaz09-_");
}

#[tokio::test]
async fn append_mismatch_falls_back_to_full_session_save() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save("append-1", "first", &["one"]))
            .await
            .unwrap(),
        1,
    );

    let persisted = repository
        .save(SessionSave {
            meta: meta("append-1", "second", 2),
            store: store_with(&["one", "two"]),
            write_mode: SessionWriteMode::Append {
                expected_existing: 0,
            },
            generation: 2,
        })
        .await
        .unwrap();

    assert_saved(persisted, 2);
    assert_eq!(repository.load("append-1").await.unwrap().len(), 2);
}

#[tokio::test]
async fn append_to_missing_transcript_falls_back_to_full_save() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());

    assert_saved(
        repository
            .save(SessionSave {
                meta: meta("append-missing", "missing", 1),
                store: store_with(&["one"]),
                write_mode: SessionWriteMode::Append {
                    expected_existing: 0,
                },
                generation: 1,
            })
            .await
            .unwrap(),
        1,
    );
    assert_eq!(repository.load("append-missing").await.unwrap().len(), 1);
}

#[tokio::test]
async fn append_to_headerless_transcript_falls_back_to_full_save() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("append-headerless.jsonl"), b"{}\n").unwrap();

    assert_saved(
        repository
            .save(SessionSave {
                meta: meta("append-headerless", "headerless", 1),
                store: store_with(&["one"]),
                write_mode: SessionWriteMode::Append {
                    expected_existing: 0,
                },
                generation: 1,
            })
            .await
            .unwrap(),
        1,
    );
    assert_eq!(repository.load("append-headerless").await.unwrap().len(), 1);
}

#[tokio::test]
async fn matching_append_watermark_persists_only_the_new_tail() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save("append-2", "first", &["one"]))
            .await
            .unwrap(),
        1,
    );
    assert_saved(
        repository
            .save(SessionSave {
                meta: meta("append-2", "second", 2),
                store: store_with(&["one", "two"]),
                write_mode: SessionWriteMode::Append {
                    expected_existing: 1,
                },
                generation: 2,
            })
            .await
            .unwrap(),
        2,
    );

    assert_eq!(repository.load("append-2").await.unwrap().len(), 2);
}

#[tokio::test]
async fn newer_snapshot_supersedes_delayed_older_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    let older_request = save_with_generation("ordered-1", "older", &["one"], 1);
    let newer_request = save_with_generation("ordered-1", "newer", &["one", "two"], 2);

    assert_saved(repository.save(newer_request).await.unwrap(), 2);
    assert_eq!(
        repository.save(older_request).await.unwrap(),
        SessionSaveOutcome::Superseded
    );

    assert_eq!(repository.load("ordered-1").await.unwrap().len(), 2);
    assert_eq!(repository.list().unwrap()[0].updated_at, 2);
}

#[tokio::test]
async fn reopened_session_continues_generation_and_rejects_queued_old_save() {
    let dir = tempfile::tempdir().unwrap();
    let original_tab = SessionRepository::new(dir.path().to_path_buf());
    let old_reservation = original_tab
        .reserve_save("reopen-1", SessionWriteMode::Full)
        .unwrap();
    let queued_old = save_with_generation(
        "reopen-1",
        "old tab",
        &["old snapshot"],
        old_reservation.generation,
    );

    let reopened_tab = original_tab.clone();
    let new_reservation = reopened_tab
        .reserve_save("reopen-1", SessionWriteMode::Full)
        .unwrap();
    assert!(new_reservation.generation > old_reservation.generation);
    let reopened_save = save_with_generation(
        "reopen-1",
        "reopened tab",
        &["old snapshot", "new message"],
        new_reservation.generation,
    );

    assert_saved(reopened_tab.save(reopened_save).await.unwrap(), 2);
    assert_eq!(
        original_tab.save(queued_old).await.unwrap(),
        SessionSaveOutcome::Superseded
    );
    assert_eq!(reopened_tab.load("reopen-1").await.unwrap().len(), 2);
}

#[tokio::test]
async fn newer_compaction_shrink_supersedes_older_large_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save_with_generation(
                "compact-1",
                "before",
                &["one", "two", "three"],
                1,
            ))
            .await
            .unwrap(),
        3,
    );
    let older_normal = save_with_generation(
        "compact-1",
        "older normal",
        &["one", "two", "three", "four"],
        2,
    );
    let newer_compact = save_with_generation("compact-1", "compacted", &["summary"], 3);

    assert_saved(repository.save(newer_compact).await.unwrap(), 1);
    assert_eq!(
        repository.save(older_normal).await.unwrap(),
        SessionSaveOutcome::Superseded
    );

    assert_eq!(repository.load("compact-1").await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn append_after_queued_compaction_preserves_the_compacted_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save_with_generation(
                "compact-append-race",
                "before",
                &["old prefix"],
                1,
            ))
            .await
            .unwrap(),
        1,
    );

    let index_lock = AdvisoryFileLock::acquire(&repository.index_path()).unwrap();

    let compact_reservation = repository
        .reserve_save("compact-append-race", SessionWriteMode::Full)
        .unwrap();
    let compact_start = Arc::new(tokio::sync::Barrier::new(2));
    let compact_task = {
        let repository = repository.clone();
        let start = compact_start.clone();
        tokio::spawn(async move {
            start.wait().await;
            repository
                .save(save_with_generation(
                    "compact-append-race",
                    "compacted",
                    &["summary"],
                    compact_reservation.generation,
                ))
                .await
        })
    };
    compact_start.wait().await;

    let append_reservation = repository
        .reserve_save(
            "compact-append-race",
            SessionWriteMode::Append {
                expected_existing: 1,
            },
        )
        .unwrap();
    assert_eq!(append_reservation.write_mode, SessionWriteMode::Full);
    let latest_store = store_with(&["summary", "new tail"]);
    let append_start = Arc::new(tokio::sync::Barrier::new(2));
    let append_task = {
        let repository = repository.clone();
        let start = append_start.clone();
        let store = latest_store.clone();
        tokio::spawn(async move {
            start.wait().await;
            repository
                .save(SessionSave {
                    meta: meta(
                        "compact-append-race",
                        "latest",
                        append_reservation.generation,
                    ),
                    store,
                    write_mode: append_reservation.write_mode,
                    generation: append_reservation.generation,
                })
                .await
        })
    };
    append_start.wait().await;
    drop(index_lock);

    assert_eq!(
        compact_task.await.unwrap().unwrap(),
        SessionSaveOutcome::Superseded
    );
    assert_saved(append_task.await.unwrap().unwrap(), 2);
    let loaded = repository.load("compact-append-race").await.unwrap();
    assert_eq!(
        loaded.iter().collect::<Vec<_>>(),
        latest_store.iter().collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn current_thread_double_writer_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    let older = repository.clone();
    let newer = repository.clone();
    let writes = async move {
        tokio::join!(
            older.save(save_with_generation("single-thread", "old", &["one"], 1)),
            newer.save(save_with_generation(
                "single-thread",
                "new",
                &["one", "two"],
                2,
            )),
        )
    };

    let (older, newer) = tokio::time::timeout(std::time::Duration::from_secs(5), writes)
        .await
        .expect("writers must not deadlock");
    assert!(matches!(
        older.unwrap(),
        SessionSaveOutcome::Saved { .. } | SessionSaveOutcome::Superseded
    ));
    assert_saved(newer.unwrap(), 2);
    assert_eq!(repository.load("single-thread").await.unwrap().len(), 2);
}

#[tokio::test]
async fn corrupt_index_is_never_replaced_with_a_default() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    let index_path = sessions_dir.join("index.json");
    let corrupt = b"{not valid json";
    std::fs::write(&index_path, corrupt).unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());

    assert!(matches!(
        repository
            .save(save("corrupt-1", "must fail", &["message"]))
            .await,
        Err(CoreError::Json(_))
    ));

    assert_eq!(std::fs::read(index_path).unwrap(), corrupt);
    assert!(!sessions_dir.join("corrupt-1.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_id_save_delete_never_leaves_an_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let repository = SessionRepository::new(dir.path().to_path_buf());
    assert_saved(
        repository
            .save(save_with_generation("race-1", "first", &["one"], 1))
            .await
            .unwrap(),
        1,
    );
    let saver = repository.clone();
    let deleter = repository.clone();

    let (save_result, delete_result) = tokio::join!(
        saver.save(save_with_generation("race-1", "second", &["one", "two"], 2,)),
        deleter.delete("race-1"),
    );
    assert_saved(save_result.unwrap(), 2);
    delete_result.unwrap();

    let indexed = repository
        .list()
        .unwrap()
        .iter()
        .any(|meta| meta.id == "race-1");
    let transcript = dir.path().join("sessions/race-1.jsonl").exists();
    assert_eq!(indexed, transcript);
}
