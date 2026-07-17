use std::collections::BTreeMap;

use zode_app_model::{
    apply_session_runtime_options, integration_catalog, LoadState, ProjectState, TranscriptItem,
    TranscriptState, ZodeAppState,
};
use zode_app_runtime::{workspace_uri_to_path, AppStateFile};
use zode_node_protocol::{
    AgentEndpoint, AgentQuery, AgentSnapshot, CapabilityManifest, HistoryItem, SessionLocator,
    ThreadHistory, ThreadSummary, WorkspaceUri,
};

pub(super) async fn load_initial_state(
    endpoint: &dyn AgentEndpoint,
    capabilities: CapabilityManifest,
    startup_workspace: WorkspaceUri,
    projectless_workspace_root: WorkspaceUri,
    persisted: Option<&AppStateFile>,
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
    state.projectless_workspace_root = Some(projectless_workspace_root);
    state.host.node_id = capabilities.node_id;
    state.host.capabilities = capabilities;
    state
        .composer
        .model
        .clone_from(&runtime_options.active_model);
    state.composer.effort.clone_from(&runtime_options.effort);
    state
        .composer
        .available_models
        .clone_from(&runtime_options.models);
    state.composer.approval_mode = runtime_options.approval_mode;
    state.composer.sandbox_mode = runtime_options.sandbox_mode;
    state.composer.sandbox_network = runtime_options.sandbox_network;
    state.composer.sandbox_label = match runtime_options.approval_mode {
        zode_node_protocol::ApprovalMode::Request => "请求批准",
        zode_node_protocol::ApprovalMode::Auto => "替我审批",
        zode_node_protocol::ApprovalMode::Full => "完全访问",
    }
    .into();
    state.composer_defaults = Some(runtime_options);
    state.threads = threads;
    if let Some(persisted) = persisted {
        state.hydrate_thread_affiliations(
            &persisted.thread_workspace_root_hints,
            &persisted.projectless_session_ids,
        );
    }
    restore_projectless_workspaces(&state);
    state.transcripts = load_transcripts(endpoint, &state.threads).await;
    state.projects = projects_from_threads(&state, &startup_workspace);
    if !state.is_projectless_workspace(&startup_workspace)
        && !state
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
    state.current_session = newest_available_session(&state);
    state.active_workspace = match state.current_session.as_ref() {
        Some(session) => state
            .project_workspace_for_session(session)
            .filter(|workspace_uri| state.available_workspace(workspace_uri))
            .cloned(),
        None => state
            .projects
            .iter()
            .find(|project| project.available)
            .map(|project| project.workspace_uri.clone()),
    };
    load_integrations(endpoint, &mut state).await;
    load_session_runtime_options(endpoint, &mut state).await;
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
            Ok(AgentSnapshot::ProjectPermissions(tools)) => {
                state
                    .project_permissions
                    .insert(workspace_uri, LoadState::Ready(tools));
            }
            Ok(_) => {
                state.project_permissions.insert(
                    workspace_uri,
                    LoadState::Failed(
                        "the endpoint returned the wrong project-permissions snapshot".into(),
                    ),
                );
            }
            Err(error) => {
                eprintln!("zode-app: project permissions could not be loaded: {error}");
                state
                    .project_permissions
                    .insert(workspace_uri, LoadState::Failed(error.to_string()));
            }
        }
    }
    Ok(state)
}

fn restore_projectless_workspaces(state: &ZodeAppState) {
    let Some(root_uri) = state.projectless_workspace_root.as_ref() else {
        return;
    };
    let Ok(root) = workspace_uri_to_path(root_uri) else {
        return;
    };
    let Ok(canonical_root) = root.canonicalize() else {
        return;
    };
    for thread in &state.threads {
        if !state.is_projectless_thread(thread) {
            continue;
        }
        let Ok(workspace) = workspace_uri_to_path(&thread.workspace_uri) else {
            continue;
        };
        let owned = workspace
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .as_deref()
            == Some(canonical_root.as_path())
            && workspace
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(&thread.session.session_id));
        if owned && std::fs::create_dir_all(&workspace).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
}

async fn load_integrations(endpoint: &dyn AgentEndpoint, state: &mut ZodeAppState) {
    let Some(workspace_uri) = state.active_available_workspace().cloned() else {
        return;
    };
    state.presentation.integrations = LoadState::Loading;
    state.presentation.integrations = match endpoint
        .query(AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        })
        .await
    {
        Ok(AgentSnapshot::Integrations(snapshot)) if snapshot.workspace_uri == workspace_uri => {
            LoadState::Ready(integration_catalog(snapshot))
        }
        Ok(AgentSnapshot::Integrations(_)) => {
            LoadState::Failed("the endpoint returned integrations for the wrong workspace".into())
        }
        Ok(_) => LoadState::Failed("the endpoint returned the wrong integrations snapshot".into()),
        Err(error) => {
            eprintln!("zode-app: integrations could not be loaded: {error}");
            LoadState::Failed(error.to_string())
        }
    };
}

async fn load_session_runtime_options(endpoint: &dyn AgentEndpoint, state: &mut ZodeAppState) {
    for session in state
        .threads
        .iter()
        .map(|thread| thread.session.clone())
        .collect::<Vec<_>>()
    {
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .runtime_options = LoadState::Loading;
        let result = endpoint
            .query(AgentQuery::SessionRuntimeOptions {
                session: session.clone(),
            })
            .await;
        match result {
            Ok(AgentSnapshot::SessionRuntimeOptions {
                session: snapshot_session,
                options,
            }) if snapshot_session == session => {
                let _ = apply_session_runtime_options(state, session, options);
            }
            Ok(AgentSnapshot::SessionRuntimeOptions { .. }) => {
                state
                    .presentation
                    .sessions
                    .entry(session)
                    .or_default()
                    .runtime_options = LoadState::Failed(
                    "the endpoint returned runtime options for the wrong session".into(),
                );
            }
            Ok(_) => {
                state
                    .presentation
                    .sessions
                    .entry(session)
                    .or_default()
                    .runtime_options = LoadState::Failed(
                    "the endpoint returned the wrong runtime-options snapshot".into(),
                );
            }
            Err(error) => {
                eprintln!("zode-app: session runtime options could not be loaded: {error}");
                state
                    .presentation
                    .sessions
                    .entry(session)
                    .or_default()
                    .runtime_options = LoadState::Failed(error.to_string());
            }
        }
    }
}

fn newest_available_session(state: &ZodeAppState) -> Option<SessionLocator> {
    state
        .threads
        .iter()
        .filter(|thread| {
            state.is_projectless_thread(thread)
                || state
                    .project_workspace_for_thread(thread)
                    .is_some_and(|workspace| state.available_workspace(workspace))
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
            // History does not persist a per-message send time or a stored
            // reaction, so restored items start at the same "unknown" state
            // a fresh message would use before either is set.
            HistoryItem::UserText { text } => TranscriptItem::user_text(text),
            HistoryItem::AssistantText { text } => TranscriptItem::assistant_text(text),
            HistoryItem::Thinking { text } => TranscriptItem::Thinking(text),
            HistoryItem::Tool { tool } => TranscriptItem::Tool(tool),
            HistoryItem::Status { code, message } => TranscriptItem::Status { code, message },
            HistoryItem::Error { message, retryable } => {
                TranscriptItem::Error { message, retryable }
            }
        })
        .collect();
    let mut transcript = TranscriptState {
        items,
        ..TranscriptState::default()
    };
    transcript.restore_historical_turns();
    transcript
}

fn projects_from_threads(
    state: &ZodeAppState,
    startup_workspace: &WorkspaceUri,
) -> Vec<ProjectState> {
    let mut projects = BTreeMap::<WorkspaceUri, i64>::new();
    for thread in &state.threads {
        let Some(project_workspace) = state.project_workspace_for_thread(thread) else {
            continue;
        };
        projects
            .entry(project_workspace.clone())
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
    use async_trait::async_trait;
    use std::collections::BTreeSet;
    use std::fs;

    use zode_app_model::{TranscriptItem, TranscriptTurnStatus};
    use zode_app_runtime::path_to_workspace_uri;
    use zode_node_protocol::{
        AgentEventStream, EndpointError, EndpointErrorKind, HistoryItem, NodeId, RuntimeOptions,
        SandboxMode, SessionLocator, ThreadHistory, ThreadStatus, ThreadSummary, ToolCall,
        ToolStatus, WorkspaceUri,
    };

    struct BootstrapEndpoint {
        threads: Vec<ThreadSummary>,
        good_session: SessionLocator,
        options: RuntimeOptions,
        integrations: Vec<zode_node_protocol::IntegrationRegistryEntry>,
    }

    #[async_trait]
    impl AgentEndpoint for BootstrapEndpoint {
        async fn command(
            &self,
            _command: zode_node_protocol::AgentCommand,
        ) -> Result<(), EndpointError> {
            unreachable!("bootstrap never sends commands")
        }

        async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
            match query {
                AgentQuery::Threads => Ok(AgentSnapshot::Threads(self.threads.clone())),
                AgentQuery::RuntimeOptions => Ok(AgentSnapshot::RuntimeOptions(
                    default_bootstrap_runtime_options(),
                )),
                AgentQuery::History { session } => Ok(AgentSnapshot::History(ThreadHistory {
                    session,
                    items: Vec::new(),
                })),
                AgentQuery::SessionRuntimeOptions { session } if session == self.good_session => {
                    Ok(AgentSnapshot::SessionRuntimeOptions {
                        session,
                        options: self.options.clone(),
                    })
                }
                AgentQuery::SessionRuntimeOptions { .. } => Err(EndpointError {
                    kind: EndpointErrorKind::Unavailable,
                    message: "session settings unavailable".into(),
                }),
                AgentQuery::ProjectPermissions { .. } => {
                    Ok(AgentSnapshot::ProjectPermissions(Vec::new()))
                }
                AgentQuery::Integrations { workspace_uri } => Ok(AgentSnapshot::Integrations(
                    zode_node_protocol::IntegrationRegistrySnapshot {
                        workspace_uri,
                        entries: self.integrations.clone(),
                        directory_error: Some("directory unavailable".into()),
                    },
                )),
                AgentQuery::Capabilities
                | AgentQuery::Diff { .. }
                | AgentQuery::InstalledPlugins
                | AgentQuery::PluginTrustReview { .. } => unreachable!(),
            }
        }

        async fn subscribe(&self) -> Result<AgentEventStream, EndpointError> {
            Ok(Box::pin(futures_util::stream::empty()))
        }
    }

    fn default_bootstrap_runtime_options() -> RuntimeOptions {
        RuntimeOptions {
            models: vec!["global-model".into()],
            active_model: Some("global-model".into()),
            effort: None,
            approval_mode: Default::default(),
            sandbox_mode: SandboxMode::Off,
            sandbox_network: false,
        }
    }

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

        assert!(
            matches!(&transcript.items[0], TranscriptItem::UserText { text, .. } if text == "user")
        );
        assert!(matches!(
            &transcript.items[1],
            TranscriptItem::AssistantText { text, .. } if text == "assistant"
        ));
        assert!(
            matches!(&transcript.items[2], TranscriptItem::Thinking(text) if text == "thinking")
        );
        assert!(matches!(&transcript.items[3], TranscriptItem::Tool(tool) if tool.id == "tool"));
        assert!(
            matches!(&transcript.items[4], TranscriptItem::Status { message, .. } if message == "done")
        );
        assert!(!transcript.busy);
        assert!(transcript.follow_tail);
        assert_eq!(transcript.turns.len(), 1);
        assert_eq!(transcript.turns[0].turn_id, None);
        assert_eq!(transcript.turns[0].start_item_index, 0);
        assert_eq!(transcript.turns[0].response_item_index, 1);
        assert_eq!(transcript.turns[0].end_item_index, Some(5));
        assert_eq!(transcript.turns[0].status, TranscriptTurnStatus::Restored);
        assert_eq!(transcript.turns[0].elapsed, None);
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
        let projectless_root = root.join("task-workspaces");
        let projectless_uri = path_to_workspace_uri(&projectless_root.join("session-1")).unwrap();
        let threads = vec![
            thread(existing_uri.clone(), "existing", 5),
            thread(missing_uri.clone(), "missing", 4),
            thread(regular_file_uri.clone(), "file", 3),
            thread(remote_uri.clone(), "remote", 2),
            thread(startup_uri.clone(), "startup", 1),
            thread(projectless_uri.clone(), "projectless", 6),
        ];

        let mut state = zode_app_model::demo_state();
        state.projectless_workspace_root = Some(path_to_workspace_uri(&projectless_root).unwrap());
        state.threads = threads;
        let projects = projects_from_threads(&state, &startup_uri);
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
        assert!(projects
            .iter()
            .all(|project| project.workspace_uri != projectless_uri));
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

        let mut state = zode_app_model::demo_state();
        state.threads = vec![newest_missing, older_available, newer_available.clone()];
        state.projects = projects;

        assert_eq!(
            newest_available_session(&state),
            Some(newer_available.session)
        );
    }

    #[test]
    fn startup_recognizes_projectless_sessions_without_exposing_a_project() {
        let root = std::env::temp_dir().join(format!(
            "zode-bootstrap-projectless-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let projectless_root = path_to_workspace_uri(&root.join("task-workspaces")).unwrap();
        let projectless = path_to_workspace_uri(&root.join("task-workspaces/session-1")).unwrap();
        let project = path_to_workspace_uri(&root.join("project")).unwrap();
        let project_session = thread(project.clone(), "project", 10);
        let projectless_session = thread(projectless.clone(), "projectless", 20);
        let mut state = zode_app_model::demo_state();
        state.projectless_workspace_root = Some(projectless_root);
        state.threads = vec![project_session, projectless_session.clone()];
        state.projects = vec![ProjectState {
            workspace_uri: project,
            expanded: true,
            available: true,
            last_opened_ms: 10,
        }];

        assert_eq!(
            newest_available_session(&state),
            Some(projectless_session.session)
        );
        assert!(
            projects_from_threads(&state, &WorkspaceUri::new("file:///startup").unwrap())
                .iter()
                .all(|candidate| candidate.workspace_uri != projectless)
        );
    }

    #[tokio::test]
    async fn bootstrap_preserves_per_session_runtime_and_known_empty_permissions() {
        let node_id = NodeId::new();
        let workspace = WorkspaceUri::new("file:///repo/bootstrap-runtime").unwrap();
        let current = SessionLocator::new(node_id, "current");
        let failed = SessionLocator::new(node_id, "failed");
        let threads = vec![
            ThreadSummary {
                session: current.clone(),
                workspace_uri: workspace.clone(),
                title: "current".into(),
                updated_at_ms: 20,
                status: ThreadStatus::Idle,
            },
            ThreadSummary {
                session: failed.clone(),
                workspace_uri: workspace.clone(),
                title: "failed".into(),
                updated_at_ms: 10,
                status: ThreadStatus::Idle,
            },
        ];
        let canonical = RuntimeOptions {
            models: vec!["session-model".into()],
            active_model: Some("session-model".into()),
            effort: Some("high".into()),
            approval_mode: Default::default(),
            sandbox_mode: SandboxMode::ReadOnly,
            sandbox_network: true,
        };
        let endpoint = BootstrapEndpoint {
            threads,
            good_session: current.clone(),
            options: canonical.clone(),
            integrations: Vec::new(),
        };
        let state = load_initial_state(
            &endpoint,
            CapabilityManifest {
                node_id,
                capabilities: BTreeSet::new(),
            },
            workspace.clone(),
            WorkspaceUri::new("file:///tmp/zode-bootstrap-task-workspaces").unwrap(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(state.current_session.as_ref(), Some(&current));
        assert_eq!(
            state.presentation.sessions[&current].runtime_options,
            LoadState::Ready(canonical)
        );
        assert!(matches!(
            state.presentation.sessions[&failed].runtime_options,
            LoadState::Failed(_)
        ));
        assert_eq!(state.composer.model.as_deref(), Some("session-model"));
        assert_eq!(state.composer.effort.as_deref(), Some("high"));
        assert_eq!(state.composer.sandbox_label, "请求批准");
        assert_eq!(
            state.project_permissions[&workspace],
            LoadState::Ready(Vec::new())
        );
    }

    #[tokio::test]
    async fn bootstrap_restores_projectless_session_without_exposing_its_scratch_project() {
        let node_id = NodeId::new();
        let startup = WorkspaceUri::new("file:///repo/bootstrap-startup").unwrap();
        let base = std::env::temp_dir().join(format!(
            "zode-bootstrap-restore-projectless-{}",
            uuid::Uuid::new_v4()
        ));
        let projectless_root_path = base.join("task-workspaces");
        std::fs::create_dir_all(&projectless_root_path).unwrap();
        let scratch_path = projectless_root_path.join("projectless-session");
        let projectless_root = path_to_workspace_uri(&projectless_root_path).unwrap();
        let scratch = path_to_workspace_uri(&scratch_path).unwrap();
        let session = SessionLocator::new(node_id, "projectless-session");
        let options = default_bootstrap_runtime_options();
        let endpoint = BootstrapEndpoint {
            threads: vec![ThreadSummary {
                session: session.clone(),
                workspace_uri: scratch.clone(),
                title: "projectless".into(),
                updated_at_ms: 20,
                status: ThreadStatus::Idle,
            }],
            good_session: session.clone(),
            options,
            integrations: Vec::new(),
        };

        let state = load_initial_state(
            &endpoint,
            CapabilityManifest {
                node_id,
                capabilities: BTreeSet::new(),
            },
            startup.clone(),
            projectless_root.clone(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(state.current_session.as_ref(), Some(&session));
        assert_eq!(state.active_workspace, None);
        assert_eq!(state.projectless_workspace_root, Some(projectless_root));
        assert!(state.is_projectless_workspace(&scratch));
        assert_eq!(state.projects.len(), 1);
        assert_eq!(state.projects[0].workspace_uri, startup);
        assert!(scratch_path.is_dir());
        let _ = std::fs::remove_dir_all(base);
    }

    #[tokio::test]
    async fn integrations_truthfulness_survives_production_bootstrap() {
        use zode_node_protocol::{
            IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistryState,
        };

        let node_id = NodeId::new();
        let workspace = WorkspaceUri::new("file:///repo/bootstrap-integrations").unwrap();
        let mut integrations = [
            "filesystem",
            "search",
            "shell",
            "git",
            "web",
            "notebook",
            "todo",
            "subagent",
            "op",
            "browser",
        ]
        .into_iter()
        .map(|name| IntegrationRegistryEntry {
            source_id: format!("tools:{name}"),
            name: name.into(),
            description: format!("{name} tools"),
            kind: IntegrationRegistryKind::ToolGroup,
            state: IntegrationRegistryState::Ready,
            installed: true,
        })
        .collect::<Vec<_>>();
        integrations.extend([
            IntegrationRegistryEntry {
                source_id: "capability:agent".into(),
                name: "智能体".into(),
                description: "运行 AI 任务".into(),
                kind: IntegrationRegistryKind::NodeCapability,
                state: IntegrationRegistryState::Ready,
                installed: true,
            },
            IntegrationRegistryEntry {
                source_id: "mcp:github".into(),
                name: "github".into(),
                description: "MCP server".into(),
                kind: IntegrationRegistryKind::Mcp,
                state: IntegrationRegistryState::Configured,
                installed: false,
            },
        ]);
        let endpoint = BootstrapEndpoint {
            threads: Vec::new(),
            good_session: SessionLocator::new(node_id, "unused"),
            options: default_bootstrap_runtime_options(),
            integrations,
        };

        let state = load_initial_state(
            &endpoint,
            CapabilityManifest {
                node_id,
                capabilities: BTreeSet::new(),
            },
            workspace,
            WorkspaceUri::new("file:///tmp/zode-bootstrap-task-workspaces").unwrap(),
            None,
        )
        .await
        .unwrap();
        let catalog = state.presentation.integrations.ready().unwrap();

        assert!(catalog.installed.len() >= 8);
        assert!(catalog.sections.len() >= 2);
        assert!(catalog.all_entries().count() >= 10);
        assert!(catalog
            .all_entries()
            .all(|entry| entry.source_id.is_some() && !entry.fixture_only));
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
