use zode_app_model::{
    demo_state, reduce_agent_event, reduce_navigation_command, reduce_presentation_command,
    AppCommand, ComingSoonFeature, EnvironmentSnapshot, IntegrationsTab, LoadState,
    NavigationOutcome, PresentationCommandOutcome, SecondaryPane, SessionPresentationState,
    SettingsCategory, ShellPage, ShellRoute, TranscriptState,
};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
    WorkspaceUri, PROTOCOL_VERSION,
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
