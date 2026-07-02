//! Compaction × noema glue.
//!
//! Three pieces live here: the extra summarization instructions that make
//! the compaction `<analysis>` block machine-parseable, the mapping from a
//! promoted analysis bullet to a noema candidate, and the
//! [`SessionMemoryStore`] implementation that routes those candidates
//! through noema's existing review / dedup / write-policy pipeline.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agent::compact::{
    build_post_compact_message, FileAttachment, PostCompactConfig, SessionMemoryEntry,
    SessionMemoryError, SessionMemoryKind, SessionMemoryStore,
};
use agent::hook::{HookEvent, HookOutcome, RustHookHandler};
use agent::message::{ContentBlock, Header, Message, MessageStore};

use crate::noema::{ZodeMemoryScope, ZodeNoema};
use crate::noema_extract::{ExtractedCandidate, MemoryKindHint, SensitivityHint};

/// Appended to the vendor summarization prompt (via the QueryLoop's
/// `compact_instructions` knob and the manual `/compact` path) so the
/// analysis bullets carry parseable kind prefixes.
pub const COMPACT_MEMORY_INSTRUCTIONS: &str = "\
In the <analysis> block, tag every bullet with one of these UPPERCASE prefixes:
- DECISION: a choice that was made and why (approach, tool, design).
- CONSTRAINT: a hard technical or project constraint that was discovered.
- REQUIREMENT: anything the USER explicitly asked for, preferred, or forbade.
- OBSERVATION: a durable fact discovered about the codebase or environment.
- OPEN QUESTION: something left unresolved.
Each bullet must be self-contained (include exact file paths, identifiers,
and values). Never invent details.";

/// Map one promoted analysis bullet to a noema candidate.
///
/// Spec mapping: Decision→decision 0.85, Constraint→constraint 0.85,
/// Requirement→constraint 0.85 + `user-requirement` tag, Observation→fact
/// 0.6 (queued for review under autoSafe), OpenQuestion→dropped (an
/// unresolved question is not a durable fact). Importance is a flat 0.6;
/// entities are left empty so noema's own extractor fills them.
pub fn candidate_from_entry(entry: &SessionMemoryEntry) -> Option<ExtractedCandidate> {
    let (kind, confidence, extra_tag) = match entry.kind {
        SessionMemoryKind::Decision => (MemoryKindHint::Decision, 0.85, None),
        SessionMemoryKind::Constraint => (MemoryKindHint::Constraint, 0.85, None),
        SessionMemoryKind::Requirement => {
            (MemoryKindHint::Constraint, 0.85, Some("user-requirement"))
        }
        SessionMemoryKind::Observation => (MemoryKindHint::Fact, 0.6, None),
        SessionMemoryKind::OpenQuestion => return None,
    };
    let body = entry.text.trim();
    if body.is_empty() {
        return None;
    }
    let mut tags = vec!["compact-sink".to_string()];
    if let Some(t) = extra_tag {
        tags.push(t.to_string());
    }
    Some(ExtractedCandidate {
        body: body.to_string(),
        entities: Vec::new(),
        tags,
        kind,
        scope: ZodeMemoryScope::Project,
        sensitivity: SensitivityHint::Internal,
        importance: 0.6,
        confidence,
    })
}

/// [`SessionMemoryStore`] backed by noema. The QueryLoop (auto path) and
/// `ZodeEngine::compact` (manual path) promote analysis bullets here; each
/// entry routes through [`ZodeNoema::submit_extracted`], reusing noema's
/// dedup / conflict / review-queue / write-policy governance. Best-effort
/// by contract: per-item submission failures are logged, never surfaced as
/// store errors (a failed memory write must not fail a compaction).
#[derive(Debug)]
pub struct NoemaSessionStore {
    noema: ZodeNoema,
    cwd: PathBuf,
}

impl NoemaSessionStore {
    pub fn new(noema: ZodeNoema, cwd: PathBuf) -> Self {
        Self { noema, cwd }
    }
}

#[async_trait::async_trait]
impl SessionMemoryStore for NoemaSessionStore {
    async fn append(&self, entry: SessionMemoryEntry) -> Result<(), SessionMemoryError> {
        let Some(candidate) = candidate_from_entry(&entry) else {
            return Ok(());
        };
        #[cfg(feature = "noema")]
        {
            let outcomes = self
                .noema
                .submit_extracted(std::slice::from_ref(&candidate), Some(self.cwd.as_path()));
            for outcome in outcomes {
                if let Err(err) = outcome {
                    tracing::debug!(error = %err, "compact memory sink: submit failed");
                }
            }
        }
        #[cfg(not(feature = "noema"))]
        {
            let _ = candidate;
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<SessionMemoryEntry>, SessionMemoryError> {
        // Write-only sink — recall goes through noema's own recall path.
        Ok(Vec::new())
    }
}

/// Most-recently-touched files, newest first, deduped, capped. Fed by the
/// tracker hook from every successful tool call carrying a `file_path`
/// input (Read / Edit / Write / NotebookEdit all match by shape, so new
/// file tools are covered without a name whitelist).
const RECENT_FILES_CAP: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct RecentFiles {
    inner: Arc<Mutex<VecDeque<PathBuf>>>,
}

impl RecentFiles {
    pub fn record(&self, path: PathBuf) {
        if let Ok(mut q) = self.inner.lock() {
            q.retain(|p| p != &path);
            q.push_front(path);
            q.truncate(RECENT_FILES_CAP);
        }
    }

    /// Newest-first snapshot of at most `n` paths.
    pub fn top(&self, n: usize) -> Vec<PathBuf> {
        self.inner
            .lock()
            .map(|q| q.iter().take(n).cloned().collect())
            .unwrap_or_default()
    }
}

/// Hook that (a) records the `file_path` of every successful tool call and
/// (b) latches `restore_pending` when a compaction actually replaced
/// messages, so the engine injects the restoration message at the start of
/// the next turn. Fires for both auto (QueryLoop) and manual (`/compact`)
/// paths — both run `compact_with_hooks`.
pub fn compact_tracker_hook(
    recent: RecentFiles,
    restore_pending: Arc<AtomicBool>,
) -> RustHookHandler {
    RustHookHandler::new("compact-tracker", move |event| {
        match event {
            HookEvent::AfterToolUse {
                input, ok: true, ..
            } => {
                if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                    recent.record(PathBuf::from(path));
                }
            }
            HookEvent::PostCompact { replaced_count, .. } if *replaced_count > 0 => {
                restore_pending.store(true, Ordering::SeqCst);
            }
            _ => {}
        }
        HookOutcome::Ok
    })
}

/// Goal sentence of the most recent compaction summary: the first non-empty
/// line after the `[Context summary]` marker. The vendor summarization
/// prompt mandates the summary begins with one sentence stating the overall
/// goal, so this line is the best available recall query seed.
pub fn latest_summary_goal(store: &MessageStore) -> Option<String> {
    let mut goal = None;
    for msg in store.iter() {
        if let Message::User { content, .. } = msg {
            for block in content {
                if let ContentBlock::Text { text } = block {
                    if let Some(rest) = text.strip_prefix("[Context summary]") {
                        if let Some(line) = rest.lines().find(|l| !l.trim().is_empty()) {
                            goal = Some(line.trim().to_string());
                        }
                    }
                }
            }
        }
    }
    goal
}

/// Read up to `max` of the given files from disk, in the given (newest
/// first) order. Unreadable files are silently skipped — deleted since
/// being touched, permission change, or non-UTF-8 content.
pub fn read_attachments(paths: &[PathBuf], max: usize) -> Vec<FileAttachment> {
    paths
        .iter()
        .filter_map(|p| {
            std::fs::read_to_string(p)
                .ok()
                .map(|content| FileAttachment {
                    path: p.clone(),
                    content,
                })
        })
        .take(max)
        .collect()
}

/// Assemble the one-shot restoration message: restored files (within the
/// vendor budget rules) plus an optional noema recall pack. Returns the
/// message and a short human-readable note for the UI, or `None` when
/// there is nothing at all to restore.
pub fn build_restore_message(
    files: Vec<FileAttachment>,
    recall: Option<String>,
    config: &PostCompactConfig,
) -> Option<(Message, String)> {
    let result = build_post_compact_message(files, config);
    let n_files = result.restored_paths.len();
    let has_recall = recall.is_some();
    let mut message = result.restored_message;
    if let Some(recall_text) = recall {
        let block = ContentBlock::Text {
            text: format!("[Post-compaction memory recall]\n{recall_text}"),
        };
        message = Some(match message {
            Some(Message::User {
                header,
                mut content,
            }) => {
                content.push(block);
                Message::User { header, content }
            }
            _ => Message::User {
                header: Header::new(),
                content: vec![block],
            },
        });
    }
    let message = message?;
    let note = format!(
        "post-compact restore: {n_files} file(s){}",
        if has_recall { " + memory recall" } else { "" }
    );
    Some((message, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::compact::SessionMemoryKind;

    fn entry(kind: SessionMemoryKind, text: &str) -> SessionMemoryEntry {
        SessionMemoryEntry::new(kind, text)
    }

    #[test]
    fn mapping_table_matches_spec() {
        let d = candidate_from_entry(&entry(SessionMemoryKind::Decision, "use fs4 locks")).unwrap();
        assert_eq!(d.kind, MemoryKindHint::Decision);
        assert!((d.confidence - 0.85).abs() < f32::EPSILON);
        assert_eq!(d.tags, vec!["compact-sink".to_string()]);
        assert_eq!(d.scope, ZodeMemoryScope::Project);

        let c = candidate_from_entry(&entry(SessionMemoryKind::Constraint, "must run on macOS"))
            .unwrap();
        assert_eq!(c.kind, MemoryKindHint::Constraint);
        assert!((c.confidence - 0.85).abs() < f32::EPSILON);

        let r = candidate_from_entry(&entry(
            SessionMemoryKind::Requirement,
            "王小明 asked for dark mode by default",
        ))
        .unwrap();
        assert_eq!(r.kind, MemoryKindHint::Constraint);
        assert!(r.tags.contains(&"user-requirement".to_string()));
        assert!(r.tags.contains(&"compact-sink".to_string()));

        let o = candidate_from_entry(&entry(SessionMemoryKind::Observation, "tests use tokio"))
            .unwrap();
        assert_eq!(o.kind, MemoryKindHint::Fact);
        assert!((o.confidence - 0.6).abs() < f32::EPSILON);

        assert!(candidate_from_entry(&entry(
            SessionMemoryKind::OpenQuestion,
            "should we split the file?"
        ))
        .is_none());
        assert!(candidate_from_entry(&entry(SessionMemoryKind::Decision, "   ")).is_none());
    }

    #[test]
    fn instructions_cover_all_prefixes() {
        for prefix in [
            "DECISION:",
            "CONSTRAINT:",
            "REQUIREMENT:",
            "OBSERVATION:",
            "OPEN QUESTION:",
        ] {
            assert!(
                COMPACT_MEMORY_INSTRUCTIONS.contains(prefix),
                "missing {prefix}"
            );
        }
    }

    #[test]
    fn recent_files_dedups_orders_and_caps() {
        let rf = RecentFiles::default();
        for i in 0..40 {
            rf.record(PathBuf::from(format!("/tmp/f{i}.rs")));
        }
        // Re-touch an old one → moves to front, no duplicate.
        rf.record(PathBuf::from("/tmp/f10.rs"));
        let top = rf.top(50);
        assert_eq!(top.len(), 32);
        assert_eq!(top[0], PathBuf::from("/tmp/f10.rs"));
        assert_eq!(top.iter().filter(|p| **p == top[0]).count(), 1);
        assert_eq!(rf.top(2).len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracker_hook_records_files_and_latches_compaction() {
        use agent::hook::{HookEvent, HookHandler};
        use std::sync::atomic::{AtomicBool, Ordering};

        let rf = RecentFiles::default();
        let pending = std::sync::Arc::new(AtomicBool::new(false));
        let hook = compact_tracker_hook(rf.clone(), pending.clone());

        hook.handle(&HookEvent::AfterToolUse {
            tool: "Read".into(),
            input: serde_json::json!({"file_path": "/tmp/read.rs"}),
            output: serde_json::json!({}),
            ok: true,
        })
        .await;
        // Failed calls and no-path inputs are ignored.
        hook.handle(&HookEvent::AfterToolUse {
            tool: "Edit".into(),
            input: serde_json::json!({"file_path": "/tmp/failed.rs"}),
            output: serde_json::json!({}),
            ok: false,
        })
        .await;
        hook.handle(&HookEvent::AfterToolUse {
            tool: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
            output: serde_json::json!({}),
            ok: true,
        })
        .await;
        assert_eq!(rf.top(10), vec![PathBuf::from("/tmp/read.rs")]);

        // Zero-replacement compactions (failures) do not latch.
        hook.handle(&HookEvent::PostCompact {
            pre_tokens: 10,
            post_tokens: 10,
            replaced_count: 0,
        })
        .await;
        assert!(!pending.load(Ordering::SeqCst));
        hook.handle(&HookEvent::PostCompact {
            pre_tokens: 100,
            post_tokens: 10,
            replaced_count: 4,
        })
        .await;
        assert!(pending.load(Ordering::SeqCst));
    }

    #[test]
    fn latest_summary_goal_finds_the_newest_goal_line() {
        use agent::message::{Header, Message, MessageStore};
        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![agent::message::ContentBlock::Text {
                    text: "[Context summary]\nOld goal here.\nDetails.".into(),
                }],
            })
            .unwrap();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![agent::message::ContentBlock::Text {
                    text: "[Context summary]\n\nShip the compact ladder.\nMore.".into(),
                }],
            })
            .unwrap();
        assert_eq!(
            latest_summary_goal(&store).as_deref(),
            Some("Ship the compact ladder.")
        );
        assert!(latest_summary_goal(&MessageStore::new()).is_none());
    }

    #[test]
    fn read_attachments_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.rs");
        std::fs::write(&ok, "fn main() {}").unwrap();
        let missing = dir.path().join("gone.rs");
        let out = read_attachments(&[ok.clone(), missing], 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, ok);
        assert_eq!(out[0].content, "fn main() {}");
    }

    #[test]
    fn build_restore_message_combines_files_and_recall() {
        use agent::compact::{FileAttachment, PostCompactConfig};
        use agent::message::{ContentBlock, Message};
        let files = vec![FileAttachment {
            path: PathBuf::from("/tmp/a.rs"),
            content: "fn a() {}".into(),
        }];
        // Files + recall.
        let (msg, note) = build_restore_message(
            files.clone(),
            Some("- User prefers dark mode".into()),
            &PostCompactConfig::default(),
        )
        .unwrap();
        let texts: Vec<&str> = match &msg {
            Message::User { content, .. } => content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
            other => panic!("expected user message, got {other:?}"),
        };
        assert!(texts.iter().any(|t| t.contains("/tmp/a.rs")));
        assert!(texts
            .iter()
            .any(|t| t.starts_with("[Post-compaction memory recall]")));
        assert_eq!(note, "post-compact restore: 1 file(s) + memory recall");

        // Recall only (all file reads failed).
        let (msg, note) = build_restore_message(
            Vec::new(),
            Some("- something".into()),
            &PostCompactConfig::default(),
        )
        .unwrap();
        assert!(matches!(msg, Message::User { .. }));
        assert_eq!(note, "post-compact restore: 0 file(s) + memory recall");

        // Nothing to restore.
        assert!(build_restore_message(Vec::new(), None, &PostCompactConfig::default()).is_none());
    }
}
