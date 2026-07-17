use std::collections::BTreeMap;

use zode_app_model::{ProjectState, TranscriptItem, TranscriptState, ZodeAppState};
use zode_app_runtime::workspace_uri_to_path;
use zode_node_protocol::{
    AgentEndpoint, AgentQuery, AgentSnapshot, CapabilityManifest, HistoryItem, SessionLocator,
    ThreadHistory, ThreadSummary, WorkspaceUri,
};

pub(super) async fn load_initial_state(
    endpoint: &dyn AgentEndpoint,
    capabilities: CapabilityManifest,
    startup_workspace: WorkspaceUri,
) -> Result<ZodeAppState, Box<dyn std::error::Error>> {
    let threads = match endpoint.query(AgentQuery::Threads).await? {
        AgentSnapshot::Threads(threads) => threads,
        _ => return Err("the endpoint returned the wrong thread snapshot".into()),
    };
    let runtime_options = match endpoint.query(AgentQuery::RuntimeOptions).await? {
        AgentSnapshot::RuntimeOptions(options) => options,
        _ => return Err("the endpoint returned the wrong runtime-options snapshot".into()),
    };
    let mut state = zode_app_model::demo_state();
    state.host.node_id = capabilities.node_id;
    state.host.capabilities = capabilities;
    state.composer.model = runtime_options.active_model;
    state.composer.effort = runtime_options.effort;
    state.threads = threads;
    state.transcripts = load_transcripts(endpoint, &state.threads).await;
    state.projects = projects_from_threads(&state.threads, &startup_workspace);
    if !state
        .projects
        .iter()
        .any(|project| project.workspace_uri == startup_workspace)
    {
        state.projects.push(ProjectState {
            workspace_uri: startup_workspace,
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
    }
    state.current_session = newest_available_session(&state.threads, &state.projects);
    state.active_workspace = state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .cloned()
        .or_else(|| {
            state
                .projects
                .iter()
                .find(|project| project.available)
                .map(|project| project.workspace_uri.clone())
        });
    for workspace_uri in state
        .projects
        .iter()
        .map(|project| project.workspace_uri.clone())
        .collect::<Vec<_>>()
    {
        match endpoint
            .query(AgentQuery::ProjectPermissions {
                workspace_uri: workspace_uri.clone(),
            })
            .await
        {
            Ok(AgentSnapshot::ProjectPermissions(tools)) if !tools.is_empty() => {
                state.project_permissions.insert(workspace_uri, tools);
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("zode-app: project permissions could not be loaded: {error}")
            }
        }
    }
    Ok(state)
}

fn newest_available_session(
    threads: &[ThreadSummary],
    projects: &[ProjectState],
) -> Option<SessionLocator> {
    threads
        .iter()
        .filter(|thread| {
            projects
                .iter()
                .any(|project| project.available && project.workspace_uri == thread.workspace_uri)
        })
        .max_by_key(|thread| thread.updated_at_ms)
        .map(|thread| thread.session.clone())
}

async fn load_transcripts(
    endpoint: &dyn AgentEndpoint,
    threads: &[ThreadSummary],
) -> BTreeMap<SessionLocator, TranscriptState> {
    let mut transcripts = BTreeMap::new();
    for thread in threads {
        let session = thread.session.clone();
        let transcript = match endpoint
            .query(AgentQuery::History {
                session: session.clone(),
            })
            .await
        {
            Ok(AgentSnapshot::History(history)) if history.session == session => {
                transcript_from_history(history)
            }
            Ok(AgentSnapshot::History(_)) => {
                eprintln!("zode-app: endpoint returned history for the wrong session");
                TranscriptState::default()
            }
            Ok(_) => {
                eprintln!("zode-app: endpoint returned the wrong history snapshot");
                TranscriptState::default()
            }
            Err(error) => {
                eprintln!(
                    "zode-app: history for session {} could not be loaded: {error}",
                    session.session_id
                );
                TranscriptState::default()
            }
        };
        transcripts.insert(session, transcript);
    }
    transcripts
}

fn transcript_from_history(history: ThreadHistory) -> TranscriptState {
    let items = history
        .items
        .into_iter()
        .map(|item| match item {
            HistoryItem::UserText { text } => TranscriptItem::UserText(text),
            HistoryItem::AssistantText { text } => TranscriptItem::AssistantText(text),
            HistoryItem::Thinking { text } => TranscriptItem::Thinking(text),
            HistoryItem::Tool { tool } => TranscriptItem::Tool(tool),
            HistoryItem::Status { code, message } => TranscriptItem::Status { code, message },
            HistoryItem::Error { message, retryable } => {
                TranscriptItem::Error { message, retryable }
            }
        })
        .collect();
    TranscriptState {
        items,
        ..TranscriptState::default()
    }
}

fn projects_from_threads(
    threads: &[ThreadSummary],
    startup_workspace: &WorkspaceUri,
) -> Vec<ProjectState> {
    let mut projects = BTreeMap::<WorkspaceUri, i64>::new();
    for thread in threads {
        projects
            .entry(thread.workspace_uri.clone())
            .and_modify(|updated| *updated = (*updated).max(thread.updated_at_ms))
            .or_insert(thread.updated_at_ms);
    }
    projects
        .into_iter()
        .map(|(workspace_uri, last_opened_ms)| ProjectState {
            available: &workspace_uri == startup_workspace
                || workspace_uri_to_path(&workspace_uri).is_ok_and(|path| path.is_dir()),
            workspace_uri,
            expanded: true,
            last_opened_ms,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use zode_app_model::TranscriptItem;
    use zode_app_runtime::path_to_workspace_uri;
    use zode_node_protocol::{
        HistoryItem, NodeId, SessionLocator, ThreadHistory, ThreadStatus, ThreadSummary, ToolCall,
        ToolStatus, WorkspaceUri,
    };

    #[test]
    fn persisted_history_projects_into_the_desktop_transcript_model() {
        let session = SessionLocator::new(NodeId::new(), "history");
        let transcript = transcript_from_history(ThreadHistory {
            session,
            items: vec![
                HistoryItem::UserText {
                    text: "user".into(),
                },
                HistoryItem::AssistantText {
                    text: "assistant".into(),
                },
                HistoryItem::Thinking {
                    text: "thinking".into(),
                },
                HistoryItem::Tool {
                    tool: ToolCall {
                        id: "tool".into(),
                        name: "read_file".into(),
                        status: ToolStatus::Completed,
                        summary: "read_file".into(),
                        detail: Some("result".into()),
                    },
                },
                HistoryItem::Status {
                    code: "history.progress".into(),
                    message: "done".into(),
                },
            ],
        });

        assert!(matches!(&transcript.items[0], TranscriptItem::UserText(text) if text == "user"));
        assert!(
            matches!(&transcript.items[1], TranscriptItem::AssistantText(text) if text == "assistant")
        );
        assert!(
            matches!(&transcript.items[2], TranscriptItem::Thinking(text) if text == "thinking")
        );
        assert!(matches!(&transcript.items[3], TranscriptItem::Tool(tool) if tool.id == "tool"));
        assert!(
            matches!(&transcript.items[4], TranscriptItem::Status { message, .. } if message == "done")
        );
        assert!(!transcript.busy);
        assert!(transcript.follow_tail);
    }

    #[test]
    fn historical_projects_require_existing_local_directories_except_startup() {
        let root = std::env::temp_dir().join(format!(
            "zode-bootstrap-projects-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let existing = root.join("existing");
        let missing = root.join("missing");
        let regular_file = root.join("regular-file");
        let startup = root.join("startup-missing-for-contract");
        fs::create_dir_all(&existing).unwrap();
        fs::write(&regular_file, b"not a directory").unwrap();
        let existing_uri = path_to_workspace_uri(&existing).unwrap();
        let missing_uri = path_to_workspace_uri(&missing).unwrap();
        let regular_file_uri = path_to_workspace_uri(&regular_file).unwrap();
        let startup_uri = path_to_workspace_uri(&startup).unwrap();
        let remote_uri = WorkspaceUri::new("zode-node://remote/workspace").unwrap();
        let threads = vec![
            thread(existing_uri.clone(), "existing", 5),
            thread(missing_uri.clone(), "missing", 4),
            thread(regular_file_uri.clone(), "file", 3),
            thread(remote_uri.clone(), "remote", 2),
            thread(startup_uri.clone(), "startup", 1),
        ];

        let projects = projects_from_threads(&threads, &startup_uri);
        let available = |uri: &WorkspaceUri| {
            projects
                .iter()
                .find(|project| &project.workspace_uri == uri)
                .unwrap()
                .available
        };
        assert!(available(&existing_uri));
        assert!(!available(&missing_uri));
        assert!(!available(&regular_file_uri));
        assert!(!available(&remote_uri));
        assert!(available(&startup_uri));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_selects_the_newest_thread_with_an_available_workspace() {
        let available = WorkspaceUri::new("file:///repo/available").unwrap();
        let missing = WorkspaceUri::new("file:///repo/missing").unwrap();
        let older_available = thread(available.clone(), "available-old", 10);
        let newer_available = thread(available.clone(), "available-new", 20);
        let newest_missing = thread(missing.clone(), "missing-newest", 30);
        let projects = vec![
            ProjectState {
                workspace_uri: missing,
                expanded: true,
                available: false,
                last_opened_ms: 30,
            },
            ProjectState {
                workspace_uri: available,
                expanded: true,
                available: true,
                last_opened_ms: 20,
            },
        ];

        assert_eq!(
            newest_available_session(
                &[newest_missing, older_available, newer_available.clone()],
                &projects,
            ),
            Some(newer_available.session)
        );
    }

    fn thread(workspace_uri: WorkspaceUri, id: &str, updated_at_ms: i64) -> ThreadSummary {
        ThreadSummary {
            session: SessionLocator::new(NodeId::new(), id),
            workspace_uri,
            title: id.into(),
            updated_at_ms,
            status: ThreadStatus::Idle,
        }
    }
}
