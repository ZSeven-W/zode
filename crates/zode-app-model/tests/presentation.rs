use zode_app_model::{
    apply_session_runtime_options, demo_state, environment_sections, reduce_agent_event,
    reduce_navigation_command, reduce_presentation_command, AppCommand, AttachmentMetadata,
    ComingSoonFeature, EnvironmentSectionKind, EnvironmentSnapshot, FileArtifact, IntegrationsTab,
    LoadState, NavigationOutcome, PresentationCommandOutcome, PreviewState, PreviewTarget,
    SecondaryPane, SessionDiffState, SessionPresentationState, SettingsCategory, ShellPage,
    ShellRoute, TranscriptItem, TranscriptState,
};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, DiffFile, DiffFileStatus, DiffSnapshot, RuntimeOptions,
    SandboxMode, SessionLocator, ThreadStatus, ThreadSummary, TurnId, WorkspaceUri,
    PROTOCOL_VERSION,
};

fn add_session(
    state: &mut zode_app_model::ZodeAppState,
    id: &str,
    workspace: &str,
) -> SessionLocator {
    let session = SessionLocator::new(state.host.node_id, id);
    let workspace_uri = WorkspaceUri::new(workspace).unwrap();
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: id.into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri,
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    session
}

fn ready_presentation(
    session: &SessionLocator,
    workspace: &str,
    branch: &str,
) -> SessionPresentationState {
    SessionPresentationState {
        diff: zode_app_model::SessionDiffState {
            dirty: false,
            load: LoadState::Ready(DiffSnapshot {
                session: session.clone(),
                files: Vec::new(),
                unified: String::new(),
            }),
        },
        context: LoadState::Ready(EnvironmentSnapshot {
            workspace_uri: WorkspaceUri::new(workspace).unwrap(),
            branch: Some(branch.into()),
            subagents: Vec::new(),
            background_processes: Vec::new(),
            sources: Vec::new(),
        }),
        preview: PreviewState::Idle,
        runtime_options: LoadState::Idle,
    }
}

#[test]
fn routes_carry_their_selected_destination() {
    let mut state = demo_state();

    assert_eq!(state.presentation.route, ShellRoute::Conversation);
    assert_eq!(state.shell.page, ShellPage::Conversation);

    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::Navigate(ShellRoute::Settings(SettingsCategory::Appearance)),
        ),
        PresentationCommandOutcome::Applied,
    );
    assert_eq!(
        state.presentation.route,
        ShellRoute::Settings(SettingsCategory::Appearance)
    );
    assert_eq!(state.shell.page, ShellPage::Settings);

    reduce_presentation_command(
        &mut state,
        AppCommand::SelectSettingsCategory(SettingsCategory::Permissions),
    );
    assert_eq!(
        state.presentation.route,
        ShellRoute::Settings(SettingsCategory::Permissions)
    );

    reduce_presentation_command(
        &mut state,
        AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
    );
    reduce_presentation_command(
        &mut state,
        AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
    );
    assert_eq!(
        state.presentation.route,
        ShellRoute::Integrations(IntegrationsTab::Skills)
    );

    reduce_presentation_command(
        &mut state,
        AppCommand::Navigate(ShellRoute::ComingSoon(ComingSoonFeature::Sites)),
    );
    assert_eq!(
        state.presentation.route,
        ShellRoute::ComingSoon(ComingSoonFeature::Sites)
    );
}

#[test]
fn secondary_panes_are_mutually_exclusive() {
    let mut state = demo_state();

    reduce_presentation_command(
        &mut state,
        AppCommand::OpenSecondary(SecondaryPane::Environment),
    );
    assert_eq!(
        state.presentation.secondary_pane,
        Some(SecondaryPane::Environment)
    );

    reduce_presentation_command(&mut state, AppCommand::OpenSecondary(SecondaryPane::Review));
    assert_eq!(
        state.presentation.secondary_pane,
        Some(SecondaryPane::Review)
    );
    assert!(state.review.open);

    reduce_presentation_command(&mut state, AppCommand::CloseSecondary);
    assert_eq!(state.presentation.secondary_pane, None);
    assert!(!state.review.open);
}

#[test]
fn leaving_conversation_closes_the_secondary_pane() {
    let mut state = demo_state();
    let routes = [
        AppCommand::Navigate(ShellRoute::Settings(SettingsCategory::General)),
        AppCommand::SelectSettingsCategory(SettingsCategory::Appearance),
        AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
    ];

    for command in routes {
        reduce_presentation_command(&mut state, AppCommand::OpenSecondary(SecondaryPane::Review));
        assert!(state.review.open);

        reduce_presentation_command(&mut state, command);

        assert_eq!(state.presentation.secondary_pane, None);
        assert!(!state.review.open);
    }
}

#[test]
fn selecting_a_session_exposes_only_that_sessions_presentation() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first", "file:///repo/first");
    let second = add_session(&mut state, "second", "file:///repo/second");
    state.presentation.sessions.insert(
        first.clone(),
        ready_presentation(&first, "file:///repo/first", "feature/first"),
    );

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(first)),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        state
            .current_session_presentation()
            .and_then(|presentation| presentation.context.ready())
            .and_then(|context| context.branch.as_deref()),
        Some("feature/first"),
    );

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(second)),
        NavigationOutcome::Applied,
    );
    let selected = state.current_session_presentation().unwrap();
    assert_eq!(selected.diff.load, LoadState::Idle);
    assert_eq!(selected.context, LoadState::Idle);
    assert!(!state.review.dirty);
}

#[test]
fn diff_invalidation_dirties_and_reloads_only_the_addressed_session() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first", "file:///repo/first");
    let second = add_session(&mut state, "second", "file:///repo/second");
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap();
    state.current_session = Some(first.clone());
    state.active_turns.insert(first.clone(), turn_id);
    state.transcripts.get_mut(&first).unwrap().busy = true;
    state.presentation.sessions.insert(
        first.clone(),
        ready_presentation(&first, "file:///repo/first", "feature/first"),
    );
    let second_ready = ready_presentation(&second, "file:///repo/second", "feature/second");
    state
        .presentation
        .sessions
        .insert(second.clone(), second_ready.clone());

    assert_eq!(
        reduce_agent_event(
            &mut state,
            AgentEvent {
                version: PROTOCOL_VERSION,
                session: first.clone(),
                turn_id,
                sequence: 1,
                kind: AgentEventKind::DiffInvalidated,
            },
        ),
        zode_app_model::ReduceOutcome::Applied,
    );

    let first_state = &state.presentation.sessions[&first];
    assert!(first_state.diff.dirty);
    assert_eq!(first_state.diff.load, LoadState::Loading);
    assert_eq!(state.presentation.sessions[&second], second_ready);
    assert!(state.review.dirty);
}

#[test]
fn background_diff_invalidation_does_not_dirty_the_legacy_review_projection() {
    let mut state = demo_state();
    let session = add_session(&mut state, "background", "file:///repo/background");
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap();
    state.active_turns.insert(session.clone(), turn_id);
    state.transcripts.get_mut(&session).unwrap().busy = true;

    reduce_agent_event(
        &mut state,
        AgentEvent {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id,
            sequence: 1,
            kind: AgentEventKind::DiffInvalidated,
        },
    );

    assert!(state.presentation.sessions[&session].diff.dirty);
    assert_eq!(
        state.presentation.sessions[&session].diff.load,
        LoadState::Loading
    );
    assert!(!state.review.dirty);
}

#[test]
fn deleting_the_current_session_clears_its_review_projection() {
    let mut state = demo_state();
    let session = add_session(&mut state, "current", "file:///repo/current");
    let mut presentation = SessionPresentationState::default();
    presentation.diff.dirty = true;
    state
        .presentation
        .sessions
        .insert(session.clone(), presentation);
    reduce_navigation_command(&mut state, AppCommand::SelectSession(session.clone()));
    reduce_presentation_command(&mut state, AppCommand::OpenSecondary(SecondaryPane::Review));

    reduce_navigation_command(
        &mut state,
        AppCommand::RequestDeleteSession(session.clone()),
    );
    reduce_navigation_command(&mut state, AppCommand::DeleteSession(session));

    assert_eq!(state.current_session, None);
    assert!(!state.review.open);
    assert!(!state.review.dirty);
    assert_eq!(state.presentation.secondary_pane, None);
}

#[test]
fn environment_sections_project_only_real_current_session_facts() {
    let mut state = demo_state();
    let session = add_session(&mut state, "current", "file:///repo/zode");
    state.current_session = Some(session.clone());
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                branch: Some("codex/zode-jian-desktop".into()),
                subagents: Vec::new(),
                background_processes: Vec::new(),
                sources: Vec::new(),
            }),
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session: session.clone(),
                    files: vec![
                        DiffFile {
                            path: "src/main.rs".into(),
                            status: DiffFileStatus::Modified,
                            additions: 7,
                            deletions: 2,
                        },
                        DiffFile {
                            path: "README.md".into(),
                            status: DiffFileStatus::Modified,
                            additions: 3,
                            deletions: 1,
                        },
                    ],
                    unified: "real diff".into(),
                }),
            },
            preview: PreviewState::Idle,
            runtime_options: LoadState::Idle,
        },
    );
    state.transcripts.get_mut(&session).unwrap().items = vec![
        TranscriptItem::FileArtifact(FileArtifact {
            id: "artifact-1".into(),
            path: "docs/report.md".into(),
            summary: "报告".into(),
            change_summary: None,
        }),
        TranscriptItem::Attachment(AttachmentMetadata {
            id: "attachment-1".into(),
            path: Some("shots/result.png".into()),
            display_name: "result.png".into(),
            media_type: "image/png".into(),
            width: Some(320),
            height: Some(180),
            byte_len: 42,
        }),
        TranscriptItem::Attachment(AttachmentMetadata {
            id: "clipboard".into(),
            path: None,
            display_name: "clipboard.png".into(),
            media_type: "image/png".into(),
            width: Some(1),
            height: Some(1),
            byte_len: 4,
        }),
    ];

    let sections = environment_sections(&state);
    assert_eq!(
        sections
            .iter()
            .map(|section| section.kind)
            .collect::<Vec<_>>(),
        vec![
            EnvironmentSectionKind::Changes,
            EnvironmentSectionKind::Host,
            EnvironmentSectionKind::Branch,
            EnvironmentSectionKind::RepositoryActions,
            EnvironmentSectionKind::Comparisons,
            EnvironmentSectionKind::Sources,
        ]
    );
    assert!(sections.iter().all(|section| !section.entries.is_empty()));
    let sources = sections
        .iter()
        .find(|section| section.kind == EnvironmentSectionKind::Sources)
        .unwrap();
    assert_eq!(
        sources
            .entries
            .iter()
            .filter_map(|entry| entry.value.as_deref())
            .collect::<Vec<_>>(),
        vec!["docs/report.md", "shots/result.png"]
    );
    assert!(!sections.iter().any(|section| matches!(
        section.kind,
        EnvironmentSectionKind::Subagents | EnvironmentSectionKind::BackgroundProcesses
    )));
}

#[test]
fn environment_sections_omit_empty_diff_branch_and_sources() {
    let mut state = demo_state();
    let session = add_session(&mut state, "empty", "file:///repo/zode");
    state.current_session = Some(session.clone());
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                branch: None,
                subagents: Vec::new(),
                background_processes: Vec::new(),
                sources: Vec::new(),
            }),
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session,
                    files: Vec::new(),
                    unified: String::new(),
                }),
            },
            preview: PreviewState::Idle,
            runtime_options: LoadState::Idle,
        },
    );

    let sections = environment_sections(&state);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].kind, EnvironmentSectionKind::Host);
    assert!(sections.iter().all(|section| !section.entries.is_empty()));
}

#[test]
fn environment_sections_reject_a_diff_snapshot_for_another_session() {
    let mut state = demo_state();
    let session = add_session(&mut state, "current", "file:///repo/zode");
    let other = SessionLocator::new(state.host.node_id, "other");
    state.current_session = Some(session.clone());
    state.presentation.sessions.insert(
        session,
        SessionPresentationState {
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session: other,
                    files: vec![DiffFile {
                        path: "other-secret.rs".into(),
                        status: DiffFileStatus::Modified,
                        additions: 99,
                        deletions: 0,
                    }],
                    unified: "other secret".into(),
                }),
            },
            ..SessionPresentationState::default()
        },
    );

    let sections = environment_sections(&state);

    assert_eq!(
        sections
            .iter()
            .map(|section| section.kind)
            .collect::<Vec<_>>(),
        vec![EnvironmentSectionKind::Host]
    );
}

#[test]
fn document_preview_is_distinct_from_diff_review() {
    assert_ne!(SecondaryPane::DocumentPreview, SecondaryPane::Review);
    let target = PreviewTarget {
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        relative_path: "docs/report.md".into(),
    };
    let failed = PreviewState::Failed {
        target: target.clone(),
        message: "not found".into(),
    };

    assert_eq!(failed.target(), Some(&target));
    assert_eq!(failed.path(), Some("docs/report.md"));
}

#[test]
fn runtime_options_are_applied_only_to_the_addressed_live_session() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first", "file:///repo/first");
    let second = add_session(&mut state, "second", "file:///repo/second");
    state
        .presentation
        .sessions
        .insert(first.clone(), SessionPresentationState::default());
    state
        .presentation
        .sessions
        .insert(second.clone(), SessionPresentationState::default());
    state.current_session = Some(first.clone());
    let options = RuntimeOptions {
        models: vec!["model-a".into(), "model-b".into()],
        active_model: Some("model-b".into()),
        effort: Some("high".into()),
        sandbox_mode: SandboxMode::ReadOnly,
        sandbox_network: true,
    };

    assert!(apply_session_runtime_options(
        &mut state,
        first.clone(),
        options.clone(),
    ));
    assert_eq!(
        state.presentation.sessions[&first].runtime_options,
        LoadState::Ready(options)
    );
    assert_eq!(
        state.presentation.sessions[&second].runtime_options,
        LoadState::Idle
    );
    assert_eq!(state.composer.model.as_deref(), Some("model-b"));
    assert_eq!(state.composer.effort.as_deref(), Some("high"));
    assert_eq!(state.composer.sandbox_label, "只读");

    let deleted = SessionLocator::new(state.host.node_id, "deleted");
    assert!(!apply_session_runtime_options(
        &mut state,
        deleted.clone(),
        RuntimeOptions {
            models: Vec::new(),
            active_model: None,
            effort: None,
            sandbox_mode: SandboxMode::Off,
            sandbox_network: false,
        },
    ));
    assert!(!state.presentation.sessions.contains_key(&deleted));
}

#[test]
fn preview_command_derives_workspace_from_the_bound_current_session() {
    let mut state = demo_state();
    let session = add_session(&mut state, "preview", "file:///repo/zode");
    state.current_session = Some(session.clone());

    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::PreviewWorkspaceFile {
                session: session.clone(),
                relative_path: "docs/report.md".into(),
            },
        ),
        PresentationCommandOutcome::Applied,
    );
    assert_eq!(
        state.presentation.secondary_pane,
        Some(SecondaryPane::DocumentPreview)
    );
    assert_eq!(
        state.presentation.sessions[&session].preview,
        PreviewState::Loading {
            target: PreviewTarget {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                relative_path: "docs/report.md".into(),
            }
        }
    );

    let other = SessionLocator::new(state.host.node_id, "other");
    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::PreviewWorkspaceFile {
                session: other,
                relative_path: "secrets.md".into(),
            },
        ),
        PresentationCommandOutcome::Ignored,
    );
}

#[test]
fn secondary_picker_state_is_mutually_exclusive_and_terminal_is_typed() {
    let mut state = demo_state();
    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::ToggleSecondaryMenu),
        PresentationCommandOutcome::Applied
    );
    assert!(state.presentation.secondary_menu_open);

    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::OpenSecondary(SecondaryPane::Terminal),
        ),
        PresentationCommandOutcome::Applied
    );
    assert_eq!(state.presentation.route, ShellRoute::Conversation);
    assert_eq!(
        state.presentation.secondary_pane,
        Some(SecondaryPane::Terminal)
    );
    assert!(!state.presentation.secondary_menu_open);
    assert!(state.terminal.open);
    assert!(state.terminal.focused);

    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CloseSecondary),
        PresentationCommandOutcome::Applied
    );
    assert_eq!(state.presentation.secondary_pane, None);
    assert!(!state.terminal.open);
    assert!(!state.terminal.focused);
}

#[test]
fn missing_desktop_contracts_have_distinct_typed_panes() {
    assert_ne!(SecondaryPane::Browser, SecondaryPane::Files);
    assert_ne!(SecondaryPane::Files, SecondaryPane::SideTask);
    assert_ne!(SecondaryPane::SideTask, SecondaryPane::DocumentPreview);
}
