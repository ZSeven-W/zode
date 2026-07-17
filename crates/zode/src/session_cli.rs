use std::path::{Path, PathBuf};

use zode_core::session_meta::SessionIndex;
use zode_core::sessions::worktree::WorktreeManager;
use zode_core::sessions::{ForkRequest, SessionStore};

use crate::args::SessionCommand;

pub async fn run(action: &SessionCommand, cwd: &Path) -> i32 {
    match execute(action, cwd).await {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            0
        }
        Err(error) => {
            eprintln!("zode: {error}");
            1
        }
    }
}

async fn execute(action: &SessionCommand, cwd: &Path) -> Result<String, zode_core::CoreError> {
    let store = SessionStore::open_default()?;
    match action {
        SessionCommand::List { json } => {
            let index = SessionIndex::load()?;
            let sessions = index.newest_first();
            if *json {
                return Ok(serde_json::to_string_pretty(&sessions)?);
            }
            Ok(sessions
                .into_iter()
                .map(|meta| {
                    format!(
                        "{}\t{}\t{}\t{}",
                        meta.id, meta.updated_at, meta.model, meta.title
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        SessionCommand::Show { id } => {
            let loaded = store.load(id).await?;
            let head = store.journal(id)?.head()?;
            let checkpoints = store.checkpoints(id)?;
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "meta": loaded.meta,
                "messageCount": loaded.messages.len(),
                "journalHead": head,
                "checkpoints": checkpoints,
            }))?)
        }
        SessionCommand::Fork {
            id,
            target_id,
            checkpoint,
            worktree,
        } => {
            let source = store.load(id).await?;
            let target_id = target_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
            let worktree_meta = if *worktree {
                Some(
                    WorktreeManager::open_default()?
                        .create(Path::new(&source.meta.cwd), &target_id)?,
                )
            } else {
                None
            };
            let result = store
                .fork(ForkRequest {
                    source_id: id.clone(),
                    target_id,
                    parent_checkpoint_id: checkpoint.clone(),
                    worktree: worktree_meta.clone(),
                })
                .await;
            let mut meta = match result {
                Ok(meta) => meta,
                Err(error) => {
                    if let Some(worktree) = &worktree_meta {
                        let _ = WorktreeManager::remove(worktree);
                    }
                    return Err(error);
                }
            };
            if let Some(worktree) = &worktree_meta {
                meta.cwd = worktree.path.clone();
                let messages = store.load(&meta.id).await?.messages;
                store.save(&meta, &messages).await?;
            }
            Ok(serde_json::to_string_pretty(&meta)?)
        }
        SessionCommand::Rewind {
            id,
            checkpoint,
            apply,
        } => {
            if *apply {
                Ok(serde_json::to_string_pretty(
                    &store.apply_rewind(id, checkpoint).await?,
                )?)
            } else {
                Ok(serde_json::to_string_pretty(
                    &store.preview_rewind(id, checkpoint)?,
                )?)
            }
        }
        SessionCommand::ApplyBack { id, target } => {
            let meta = store.load_meta(id)?;
            let worktree = meta.worktree.ok_or_else(|| {
                zode_core::CoreError::Other(format!("session {id} has no isolated worktree"))
            })?;
            let target = target
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| cwd.into());
            WorktreeManager::apply_back(&worktree, &target)?;
            store.journal(id)?.append(
                "worktree.applied_back",
                serde_json::json!({"target": target}),
            )?;
            Ok(format!("applied {} to {}", worktree.path, target.display()))
        }
        SessionCommand::Delete {
            id,
            remove_worktree,
        } => {
            let meta = store.load_meta(id).ok();
            if *remove_worktree {
                if let Some(worktree) = meta.as_ref().and_then(|meta| meta.worktree.as_ref()) {
                    WorktreeManager::remove(worktree)?;
                }
            }
            SessionIndex::delete_session_file_and_index(id).await?;
            Ok(format!("deleted session {id}"))
        }
    }
}
