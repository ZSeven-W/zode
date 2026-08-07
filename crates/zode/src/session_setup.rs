//! Session targeting and preparation shared by the headless / REPL / TUI
//! entrypoints: `--session-id` / `--resume` / `--continue` / `--fork-session`
//! resolution and transcript attachment.

use std::path::PathBuf;

use agent::message::MessageStore;
use agent::session::Session;
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::sessions::{DurableSessionMeta, ForkRequest, SessionStore};
use zode_core::ZodeEngine;

use crate::args::Args;

pub(crate) struct PreparedHeadlessSession {
    pub(crate) meta: DurableSessionMeta,
    pub(crate) messages: MessageStore,
}

pub(crate) async fn prepare_headless_session(
    args: &Args,
    prompt: &str,
    cwd: &std::path::Path,
    model: &str,
) -> Result<PreparedHeadlessSession, zode_core::CoreError> {
    let store = SessionStore::open_default()?;
    if let Some(source_id) = &args.fork_session {
        let source = store.load(source_id).await?;
        let target_id = uuid::Uuid::new_v4().simple().to_string();
        let worktree = if args.fork_worktree {
            Some(
                zode_core::sessions::worktree::WorktreeManager::open_default()?
                    .create(std::path::Path::new(&source.meta.cwd), &target_id)?,
            )
        } else {
            None
        };
        let meta_result = store
            .fork(ForkRequest {
                source_id: source_id.clone(),
                target_id,
                parent_checkpoint_id: None,
                worktree: worktree.clone(),
            })
            .await;
        let meta = match meta_result {
            Ok(mut meta) => {
                if let Some(worktree) = &worktree {
                    meta.cwd = worktree.path.clone();
                    let loaded = store.load(&meta.id).await?;
                    store.save(&meta, &loaded.messages).await?;
                }
                meta
            }
            Err(error) => {
                if let Some(worktree) = &worktree {
                    let _ = zode_core::sessions::worktree::WorktreeManager::remove(worktree);
                }
                return Err(error);
            }
        };
        let messages = store.load(&meta.id).await?.messages;
        return Ok(PreparedHeadlessSession { meta, messages });
    }

    if let Some(id) = &args.session_id {
        // Validate before any filesystem join. Exact ids never prefix-match.
        store.session_dir(id)?;
        if store.has_sidecar(id) || SessionIndex::session_path(id)?.is_file() {
            let loaded = store.load(id).await?;
            return Ok(PreparedHeadlessSession {
                meta: loaded.meta,
                messages: loaded.messages,
            });
        }
        let meta = fresh_session_meta(id.clone(), prompt, cwd, model);
        store.create(meta.clone())?;
        return Ok(PreparedHeadlessSession {
            meta,
            messages: MessageStore::new(),
        });
    }

    if args.resume.is_some() || args.continue_ {
        let resolved = resolve_resume_target_strict(args)?;
        let loaded = store.load(&resolved.id).await?;
        return Ok(PreparedHeadlessSession {
            meta: loaded.meta,
            messages: loaded.messages,
        });
    }

    let id = uuid::Uuid::new_v4().simple().to_string();
    let meta = fresh_session_meta(id, prompt, cwd, model);
    store.create(meta.clone())?;
    Ok(PreparedHeadlessSession {
        meta,
        messages: MessageStore::new(),
    })
}

pub(crate) fn fresh_session_meta(
    id: String,
    prompt: &str,
    cwd: &std::path::Path,
    model: &str,
) -> DurableSessionMeta {
    DurableSessionMeta::new(SessionMeta {
        id,
        title: zode_core::session_meta::title_from_prompt(prompt),
        cwd: cwd.display().to_string(),
        model: model.to_string(),
        updated_at: unix_now_secs(),
    })
}

pub(crate) fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn resolve_resume_target_strict(
    args: &Args,
) -> Result<SessionMeta, zode_core::CoreError> {
    let index = SessionIndex::load()?;
    resolve_resume_in_index(&index, args.resume.as_deref())
}

pub(crate) fn resolve_resume_in_index(
    index: &SessionIndex,
    resume: Option<&str>,
) -> Result<SessionMeta, zode_core::CoreError> {
    if let Some(prefix) = resume {
        // A complete id wins outright — user-chosen ids may prefix each other
        // (e.g. "abc" and "abc2"), and an exact match is never ambiguous.
        if let Some(exact) = index.sessions.iter().find(|meta| meta.id == prefix) {
            return Ok(exact.clone());
        }
        let matches = index
            .sessions
            .iter()
            .filter(|meta| meta.id.starts_with(prefix))
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [target] => Ok((*target).clone()),
            [] => Err(zode_core::CoreError::Other(format!(
                "session not found: {prefix}"
            ))),
            _ => Err(zode_core::CoreError::Other(format!(
                "session prefix is ambiguous: {prefix}"
            ))),
        };
    }
    index
        .latest()
        .cloned()
        .ok_or_else(|| zode_core::CoreError::Other("no session to continue".into()))
}

/// Resolve which session `--resume`/`--continue` targets, if any. Done BEFORE
/// engine assembly so the engine can be built in the session's own cwd.
pub(crate) fn resolve_resume_target(args: &Args) -> Option<SessionMeta> {
    let requested = args.resume.as_deref();
    if requested.is_none() && !args.continue_ {
        return None;
    }
    let index = match SessionIndex::load() {
        Ok(index) => index,
        Err(error) => {
            eprintln!("zode: could not load or repair session index: {error}");
            return None;
        }
    };
    if let Some(prefix) = requested {
        let target = index.find_prefix(prefix).cloned();
        if target.is_none() {
            eprintln!("zode: session not found: {prefix}");
        }
        target
    } else {
        index.latest().cloned()
    }
}

/// The session's recorded cwd, but only if that directory still exists (else
/// the caller falls back to the launch cwd).
pub(crate) fn resume_dir(meta: &Option<SessionMeta>) -> Option<PathBuf> {
    meta.as_ref().and_then(|m| {
        let p = PathBuf::from(&m.cwd);
        p.is_dir().then_some(p)
    })
}

/// Load the resolved session's store into `engine`. Returns the (possibly
/// updated) engine and the resumed session id.
pub(crate) async fn attach_session(
    engine: ZodeEngine,
    meta: Option<SessionMeta>,
) -> (ZodeEngine, Option<String>) {
    let Some(meta) = meta else {
        return (engine, None);
    };
    match SessionIndex::session_path(&meta.id) {
        Ok(path) => match Session::load(&path).await {
            Ok(store) => {
                let short: String = meta.id.chars().take(8).collect();
                eprintln!("zode: resumed session {short} ({})", meta.title);
                let engine = engine.with_store(store);
                // Archived originals of compaction-tombstoned messages: the
                // TUI merges them over the tombstones so the resumed
                // transcript still shows the pre-compaction conversation.
                if let Ok(sessions) = SessionStore::open_default() {
                    let archive = sessions.load_compacted_archive(&meta.id).await;
                    engine.seed_compacted_overlay(&archive);
                }
                (engine, Some(meta.id))
            }
            Err(e) => {
                eprintln!("zode: could not load session {}: {e}", meta.id);
                (engine, None)
            }
        },
        Err(_) => (engine, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_resume_prefers_an_exact_id_over_prefix_ambiguity() {
        let mut index = SessionIndex::default();
        for id in ["abc", "abc2"] {
            index.upsert(SessionMeta {
                id: id.into(),
                title: id.into(),
                cwd: "/p".into(),
                model: "m".into(),
                updated_at: 1,
            });
        }
        assert_eq!(
            resolve_resume_in_index(&index, Some("abc")).unwrap().id,
            "abc"
        );
        assert!(resolve_resume_in_index(&index, Some("ab")).is_err());
        assert!(resolve_resume_in_index(&index, Some("zzz")).is_err());
        assert!(resolve_resume_in_index(&SessionIndex::default(), None).is_err());
    }
}
