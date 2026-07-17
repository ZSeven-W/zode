//! Read-only local operations dashboard.

use std::collections::HashMap;

use agent::session::Session;
use serde::Serialize;
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::sessions::journal::{JournalEntry, JournalHead};
use zode_core::sessions::SessionStore;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSnapshot {
    schema: &'static str,
    sessions: Vec<DashboardRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardRow {
    id: String,
    title: String,
    cwd: String,
    model: String,
    updated_at: u64,
    message_count: usize,
    activity: String,
    last_event: Option<String>,
    checkpoint_count: usize,
    worktree_path: Option<String>,
    worktree_branch: Option<String>,
    parent_session_id: Option<String>,
}

pub async fn run(json: bool) -> i32 {
    match snapshot().await {
        Ok(snapshot) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
            } else {
                print_table(&snapshot.sessions);
            }
            0
        }
        Err(error) => {
            eprintln!("zode dashboard: {error}");
            1
        }
    }
}

async fn snapshot() -> Result<DashboardSnapshot, zode_core::CoreError> {
    let index = SessionIndex::load()?;
    let store = SessionStore::open_default()?;
    let mut rows = Vec::with_capacity(index.sessions.len());
    for meta in index.newest_first() {
        rows.push(row(&store, meta.clone()).await?);
    }
    Ok(DashboardSnapshot {
        schema: "zode.dashboard.v1",
        sessions: rows,
    })
}

async fn row(
    store: &SessionStore,
    index_meta: SessionMeta,
) -> Result<DashboardRow, zode_core::CoreError> {
    let message_count = Session::load(SessionIndex::session_path(&index_meta.id)?)
        .await
        .map_err(|error| zode_core::CoreError::Other(error.to_string()))?
        .len();
    if !store.has_sidecar(&index_meta.id) {
        return Ok(DashboardRow {
            id: index_meta.id,
            title: index_meta.title,
            cwd: index_meta.cwd,
            model: index_meta.model,
            updated_at: index_meta.updated_at,
            message_count,
            activity: "v1".into(),
            last_event: None,
            checkpoint_count: 0,
            worktree_path: None,
            worktree_branch: None,
            parent_session_id: None,
        });
    }
    let durable = store.load_meta(&index_meta.id)?;
    let journal = store.journal(&index_meta.id)?;
    let entries = journal.read_all()?;
    let head = journal.head()?;
    let (activity, last_event) = infer_activity(&entries, &head);
    let checkpoint_count = store.checkpoints(&index_meta.id)?.len();
    let (worktree_path, worktree_branch) = durable
        .worktree
        .as_ref()
        .map(|worktree| (Some(worktree.path.clone()), Some(worktree.branch.clone())))
        .unwrap_or_default();
    Ok(DashboardRow {
        id: durable.id,
        title: durable.title,
        cwd: durable.cwd,
        model: durable.model,
        updated_at: durable.updated_at,
        message_count,
        activity,
        last_event,
        checkpoint_count,
        worktree_path,
        worktree_branch,
        parent_session_id: durable.parent_session_id,
    })
}

/// Walk the active logical branch backwards so events abandoned by rewind do
/// not leak into the displayed state.
fn infer_activity(entries: &[JournalEntry], head: &JournalHead) -> (String, Option<String>) {
    let by_seq = entries
        .iter()
        .map(|entry| (entry.seq, entry))
        .collect::<HashMap<_, _>>();
    let mut cursor = head.logical_head;
    while let Some(seq) = cursor {
        let Some(entry) = by_seq.get(&seq) else {
            break;
        };
        if entry.kind == "run.event" {
            let event = entry
                .payload
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let activity = match event.as_str() {
                "run.completed" => entry
                    .payload
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("completed")
                    .to_string(),
                "turn.completed" => "between_turns".into(),
                "error" => "failed".into(),
                _ => "incomplete".into(),
            };
            return (activity, Some(event));
        }
        cursor = entry.parent_seq;
    }
    ("idle".into(), None)
}

fn print_table(rows: &[DashboardRow]) {
    println!("ID\tACTIVITY\tMESSAGES\tCHECKPOINTS\tWORKTREE\tMODEL\tTITLE");
    for row in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            clean(&row.id),
            clean(&row.activity),
            row.message_count,
            row.checkpoint_count,
            clean(row.worktree_branch.as_deref().unwrap_or("-")),
            clean(&row.model),
            clean(&row.title),
        );
    }
}

fn clean(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, parent: Option<u64>, kind: &str, event: &str) -> JournalEntry {
        JournalEntry {
            schema_version: 1,
            seq,
            timestamp_ms: 1,
            parent_seq: parent,
            kind: kind.into(),
            payload: serde_json::json!({"type": event, "status": "completed"}),
        }
    }

    #[test]
    fn activity_follows_the_logical_branch_after_rewind() {
        let entries = vec![
            entry(0, None, "run.event", "run.completed"),
            entry(1, Some(0), "run.event", "run.started"),
            entry(2, Some(0), "session.meta_updated", "ignored"),
        ];
        let head = JournalHead {
            next_seq: 3,
            logical_head: Some(2),
            journal_len: 0,
        };
        assert_eq!(
            infer_activity(&entries, &head),
            ("completed".into(), Some("run.completed".into()))
        );
    }
}
