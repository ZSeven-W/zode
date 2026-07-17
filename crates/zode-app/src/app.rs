use std::{collections::BTreeSet, sync::Arc};

use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::{ProjectState, ZodeAppState};
use zode_app_ui::{
    ComposerController, Insets, ProjectPickerController, SessionRenameController, TerminalGrid,
    TerminalPanelController, WidgetId, WorkspaceSnapshot,
};

use crate::services::{
    ExternalOpenService, LocalExternalOpenService, NativeSessionWindowService,
    SessionWindowService, WorkspaceService,
};
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    clipboard::{ClipboardService, NativeClipboardService},
    command_bridge::CommandBridge,
    event_bridge::AgentEventBridge,
    event_map::{
        composer_outcome_command, map_ime_input, map_keyboard, map_pointer_button,
        map_pointer_move, map_touch, map_wheel,
    },
    presentation_bridge::PresentationQueryBridge,
    render::{NativeBackend, RasterSurface},
    services::LocalTerminalService,
    terminal_runtime::TerminalRuntime,
    window_state::{
        AppWake, WindowGeometry, WindowState, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
};
use zode_app_runtime::{workspace_uri_to_path, AppStateStore, TaskContext};
use zode_node_protocol::{AgentEndpoint, NodeCapability, UserContent, WorkspaceUri};

#[path = "app/external-preview.rs"]
mod external_preview;
mod integrations;
mod interaction;
mod navigation_persistence;
mod navigation_state;
mod panel_menu;
mod persistence;
mod presentation;
mod project_picker;
mod queue;
mod queue_focus;
mod session_menu;
mod settings;
mod sidebar;
mod startup;
mod terminal;
mod window;

pub use startup::{run_demo, run_demo_for_session};

pub struct DesktopApp {
    app_state: ZodeAppState,
    a11y: Option<AccessibilityHost>,
    window: Option<Arc<Window>>,
    presenter: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    presenter_size: Option<(u32, u32)>,
    raster: Option<RasterSurface>,
    renderer: NativeBackend,
    window_state: WindowState,
    proxy: EventLoopProxy<AppWake>,
    agent_events: Option<AgentEventBridge>,
    composer: ComposerController,
    project_picker_controller: ProjectPickerController,
    session_rename_controller: SessionRenameController,
    /// Full queued payloads stay controller-private so immutable UI snapshots
    /// never retain image base64. `ZodeAppState` stores only lightweight queue
    /// previews keyed by the same session-local message id.
    queued_payloads: queue::QueuedPayloadStore,
    /// Sessions created optimistically by a first composer submit stay
    /// provisional until the command pump confirms their runtime options.
    /// Explicit queue guidance must not race ahead of that Create+Start batch.
    provisional_sessions: BTreeSet<zode_node_protocol::SessionLocator>,
    terminal_grid: TerminalGrid,
    terminal_controller: TerminalPanelController,
    terminal_runtime: TerminalRuntime,
    terminal_workspace: Option<WorkspaceUri>,
    modifiers: ModifiersState,
    command_bridge: Option<CommandBridge>,
    presentation_queries: Option<PresentationQueryBridge>,
    settings_touch: crate::input_dispatch::SettingsTouchTracker,
    frame_snapshot: WorkspaceSnapshot,
    frame_snapshot_prepared: bool,
    accessibility_tree_dirty: bool,
    focused_widget: Option<WidgetId>,
    hovered_widget: Option<WidgetId>,
    window_focused: bool,
    clipboard: Option<Arc<dyn ClipboardService>>,
    external_open: Arc<dyn ExternalOpenService>,
    session_window: Arc<dyn SessionWindowService>,
    workspace_picker: project_picker::WorkspacePickerEffect,
    app_state_store: Option<AppStateStore>,
    window_geometry: Option<WindowGeometry>,
}

impl DesktopApp {
    pub fn new(mut app_state: ZodeAppState, proxy: EventLoopProxy<AppWake>) -> Self {
        app_state.local_profile.display_name = navigation_state::local_display_name_from_env();
        let app_state_store = match AppStateStore::from_default_config() {
            Ok(store) => Some(store),
            Err(error) => {
                eprintln!("zode-app: UI state persistence is unavailable: {error}");
                None
            }
        };
        let persisted = app_state_store
            .as_ref()
            .and_then(|store| match store.load() {
                Ok(state) => Some(state),
                Err(error) => {
                    eprintln!(
                        "zode-app: persisted UI state could not be loaded and was left untouched: {error}"
                    );
                    None
                }
            });
        if let Some(persisted) = persisted.as_ref() {
            app_state.ui_preferences = persisted.ui_preferences.clone();
            app_state.sidebar.tasks_expanded = app_state.ui_preferences.sidebar_tasks_expanded;
            navigation_state::hydrate_session_navigation(&mut app_state, persisted);
            for project in &mut app_state.projects {
                project.expanded = !persisted
                    .collapsed_workspaces
                    .contains(project.workspace_uri.as_str());
            }
            match persisted.task_context.as_ref() {
                Some(TaskContext::Project { workspace_uri })
                    if app_state.is_projectless_workspace(workspace_uri) =>
                {
                    app_state.current_session = None;
                    app_state.active_workspace = None;
                    app_state.presentation.integrations = zode_app_model::LoadState::Idle;
                }
                Some(TaskContext::Project { workspace_uri }) => {
                    let context_changed =
                        app_state.active_workspace.as_ref() != Some(workspace_uri);
                    if !app_state
                        .projects
                        .iter()
                        .any(|project| &project.workspace_uri == workspace_uri)
                        && workspace_uri_to_path(workspace_uri).is_ok_and(|path| path.is_dir())
                    {
                        app_state.projects.push(ProjectState {
                            workspace_uri: workspace_uri.clone(),
                            expanded: true,
                            available: true,
                            last_opened_ms: crate::command_projection::now_ms(),
                        });
                    }
                    app_state.current_session = None;
                    app_state.active_workspace = app_state
                        .available_workspace(workspace_uri)
                        .then(|| workspace_uri.clone());
                    if context_changed {
                        app_state.presentation.integrations = zode_app_model::LoadState::Idle;
                    }
                }
                Some(TaskContext::Projectless) => {
                    app_state.current_session = None;
                    app_state.active_workspace = None;
                    app_state.presentation.integrations = zode_app_model::LoadState::Idle;
                }
                None => navigation_state::restore_last_session(
                    &mut app_state,
                    persisted.last_session.as_deref(),
                ),
            }
        }
        app_state.composer.focused = false;
        app_state.terminal.focused = false;
        if !app_state
            .host
            .capabilities
            .capabilities
            .contains(&NodeCapability::Terminal)
            && app_state.terminal.unavailable_reason.is_none()
        {
            app_state.terminal.unavailable_reason =
                Some("Terminal is unavailable on this node.".into());
        }
        let mut composer = ComposerController::new(app_state.composer.draft.clone());
        let project_picker_controller =
            ProjectPickerController::new(app_state.project_picker.search.clone());
        let busy = app_state
            .current_session
            .as_ref()
            .and_then(|session| app_state.transcripts.get(session))
            .is_some_and(|transcript| transcript.busy);
        composer.set_busy(busy);
        let terminal_proxy = proxy.clone();
        let terminal_runtime =
            TerminalRuntime::new(Arc::new(LocalTerminalService::new()), move || {
                let _ = terminal_proxy.send_event(AppWake::Redraw);
            });
        let workspace_picker = project_picker::WorkspacePickerEffect::new(proxy.clone());
        let frame_snapshot = WorkspaceSnapshot::build(
            &app_state,
            DEFAULT_WINDOW_WIDTH as f32,
            DEFAULT_WINDOW_HEIGHT as f32,
            Insets::ZERO,
        );
        let focused_widget = frame_snapshot
            .focused
            .or_else(|| frame_snapshot.focusable_ids().first().copied());
        let window_geometry = persisted.and_then(|persisted| persisted.window_geometry);
        let clipboard = match NativeClipboardService::new() {
            Ok(clipboard) => Some(Arc::new(clipboard) as Arc<dyn ClipboardService>),
            Err(error) => {
                eprintln!("zode-app: native clipboard is unavailable: {error}");
                None
            }
        };
        Self {
            app_state,
            a11y: None,
            window: None,
            presenter: None,
            presenter_size: None,
            raster: None,
            renderer: NativeBackend::new(1.0),
            window_state: WindowState::new(1221, 992, 1.0),
            proxy,
            agent_events: None,
            composer,
            project_picker_controller,
            session_rename_controller: SessionRenameController::default(),
            queued_payloads: queue::QueuedPayloadStore::default(),
            provisional_sessions: BTreeSet::new(),
            terminal_grid: TerminalGrid::new(80, 24),
            terminal_controller: TerminalPanelController::default(),
            terminal_runtime,
            terminal_workspace: None,
            modifiers: ModifiersState::empty(),
            command_bridge: None,
            presentation_queries: None,
            settings_touch: crate::input_dispatch::SettingsTouchTracker::default(),
            frame_snapshot,
            frame_snapshot_prepared: true,
            accessibility_tree_dirty: true,
            focused_widget,
            hovered_widget: None,
            window_focused: false,
            clipboard,
            external_open: Arc::new(LocalExternalOpenService),
            session_window: Arc::new(NativeSessionWindowService),
            workspace_picker,
            app_state_store,
            window_geometry,
        }
    }

    pub fn set_clipboard_service(&mut self, clipboard: Arc<dyn ClipboardService>) {
        self.clipboard = Some(clipboard);
    }

    pub fn set_external_open_service(&mut self, service: Arc<dyn ExternalOpenService>) {
        self.external_open = service;
    }

    pub fn set_session_window_service(&mut self, service: Arc<dyn SessionWindowService>) {
        self.session_window = service;
    }

    pub fn set_workspace_service(&mut self, service: Arc<dyn WorkspaceService>) {
        self.workspace_picker.set_service(service);
    }

    /// Connect a live endpoint stream to the winit wake path. Call while the
    /// application Tokio runtime is entered, before `run_app` takes control.
    pub fn attach_endpoint(&mut self, endpoint: Arc<dyn AgentEndpoint>) {
        self.agent_events = Some(AgentEventBridge::spawn(
            endpoint.clone(),
            self.proxy.clone(),
        ));
        self.command_bridge = Some(CommandBridge::spawn(endpoint.clone(), self.proxy.clone()));
        self.presentation_queries =
            Some(PresentationQueryBridge::spawn(endpoint, self.proxy.clone()));
    }

    fn sync_composer_busy(&mut self) {
        let busy = self
            .app_state
            .current_session
            .as_ref()
            .and_then(|session| self.app_state.transcripts.get(session))
            .is_some_and(|transcript| transcript.busy);
        self.composer.set_busy(busy);
    }

    fn apply_composer_outcome(&mut self, mut outcome: zode_app_ui::ComposerOutcome) {
        self.app_state.composer.draft = self.composer.text().to_owned();

        let editing = self
            .app_state
            .current_session
            .clone()
            .zip(self.app_state.composer.editing_queued_message);
        let edited_submission = match &outcome {
            zode_app_ui::ComposerOutcome::Send(submission)
            | zode_app_ui::ComposerOutcome::Queue(submission) => Some(submission),
            _ => None,
        };
        if let (Some((session, id)), Some(submission)) = (editing, edited_submission) {
            self.enqueue_command(zode_app_model::AppCommand::EditQueuedMessageText {
                session,
                id,
                text: submission_text(&submission.content),
            });
            return;
        }

        if let Some(session) = submission_queue_session(&self.app_state, &outcome) {
            let submission = match &mut outcome {
                zode_app_ui::ComposerOutcome::Send(submission)
                | zode_app_ui::ComposerOutcome::Queue(submission) => submission,
                _ => unreachable!("only sendable composer outcomes can enter a message queue"),
            };
            let command = zode_app_model::AppCommand::EnqueueMessage {
                session,
                content: std::mem::take(&mut submission.content),
                attachments: submission.attachments.clone(),
            };
            // A regular Send can arrive while the session is idle even though
            // it already owns pending queue items. Project it like Queue so it
            // cannot appear in the transcript ahead of the existing head.
            self.app_state.composer.attachments.clear();
            self.enqueue_command(command);
            return;
        }

        if matches!(outcome, zode_app_ui::ComposerOutcome::Queue(_)) {
            crate::command_bridge::project_command_error(
                &mut self.app_state,
                "cannot queue a message without an active session".into(),
            );
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }

        if let Some(command) = composer_outcome_command(&mut outcome) {
            let first_submit = matches!(&command, zode_app_model::AppCommand::Submit(_))
                && !has_dispatchable_current_session(&self.app_state);
            self.enqueue_command(command);
            if first_submit {
                self.mark_provisional_first_submit();
            }
        }
        interaction::project_composer_outcome(&mut self.app_state, &outcome);
        self.rebuild_frame_snapshot();
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

fn submission_text(content: &[UserContent]) -> String {
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::Text { text } => Some(text.as_str()),
            UserContent::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn submission_queue_session(
    state: &ZodeAppState,
    outcome: &zode_app_ui::ComposerOutcome,
) -> Option<zode_node_protocol::SessionLocator> {
    let session = state.current_session.clone()?;
    if session.session_id.starts_with("local-error-")
        || state.available_workspace_for_session(&session).is_none()
    {
        return None;
    }
    match outcome {
        zode_app_ui::ComposerOutcome::Queue(_) => Some(session),
        zode_app_ui::ComposerOutcome::Send(_)
            if state
                .message_queues
                .get(&session)
                .is_some_and(|queue| !queue.items.is_empty()) =>
        {
            Some(session)
        }
        _ => None,
    }
}

fn has_dispatchable_current_session(state: &ZodeAppState) -> bool {
    state
        .current_session
        .as_ref()
        .filter(|session| !session.session_id.starts_with("local-error-"))
        .is_some_and(|session| state.available_workspace_for_session(session).is_some())
}

impl ApplicationHandler<AppWake> for DesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.open_window(event_loop) {
            eprintln!("zode-app: failed to open window: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppWake) {
        match event {
            AppWake::Redraw => {
                self.drain_accessibility_actions();
                let workspace_picks_applied = self.drain_workspace_pick_results();
                let event_drain = self
                    .agent_events
                    .as_mut()
                    .map_or_else(Default::default, |events| {
                        events.drain_into(&mut self.app_state)
                    });
                let previous_session = self.app_state.current_session.clone();
                let previous_queue_edit = self.app_state.composer.editing_queued_message;
                let commands_applied = self
                    .command_bridge
                    .as_mut()
                    .map_or(0, |commands| commands.drain_into(&mut self.app_state));
                self.reconcile_provisional_sessions();
                self.sync_queue_editor_after_state_change(
                    previous_session.clone(),
                    previous_queue_edit,
                );
                self.prune_queued_payloads();
                for command in event_drain.queue_dispatch_commands() {
                    self.enqueue_command(command);
                }
                if previous_session != self.app_state.current_session {
                    self.request_presentation_refresh(
                        presentation::PresentationRefresh::SessionChanged,
                    );
                } else if commands_applied > 0 {
                    self.request_presentation_refresh(
                        presentation::PresentationRefresh::CommandCompleted,
                    );
                } else {
                    for session in event_drain.diff_invalidated.iter().cloned() {
                        self.request_presentation_refresh(
                            presentation::PresentationRefresh::DiffInvalidated(session),
                        );
                    }
                }
                let queries_applied = self.drain_presentation_queries();
                let terminal_changed = self.drain_terminal_output();
                let background_changed = event_drain.changed
                    || commands_applied > 0
                    || queries_applied > 0
                    || workspace_picks_applied > 0
                    || terminal_changed;
                if background_changed {
                    self.rebuild_frame_snapshot();
                }
                self.sync_composer_busy();
                if background_changed {
                    self.request_redraw();
                }
            }
            AppWake::Close => {
                self.record_window_geometry();
                self.persist_ui_state();
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.record_window_geometry();
                self.persist_ui_state();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.window_state.physical_width = size.width.max(1);
                self.window_state.physical_height = size.height.max(1);
                self.rebuild_frame_snapshot();
                self.resize_terminal_grid();
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
                self.request_redraw();
            }
            WindowEvent::Moved(_) => {
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_state.scale_factor = scale_factor;
                self.renderer = NativeBackend::new(scale_factor as f32);
                self.rebuild_frame_snapshot();
                self.resize_terminal_grid();
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
                self.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => {
                self.app_state.host.system_theme = crate::event_map::map_system_theme(Some(theme));
                self.rebuild_frame_snapshot();
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                if let Some(a11y) = self.a11y.as_mut() {
                    a11y.set_window_focused(focused);
                }
                self.sync_window_focus(focused);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(input) = map_keyboard(
                    &event.logical_key,
                    self.modifiers,
                    event.state == ElementState::Pressed,
                ) {
                    self.handle_unified_input(input);
                }
            }
            WindowEvent::Ime(event) => {
                self.handle_unified_input(map_ime_input(&event));
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_unified_input(map_pointer_move(
                    position,
                    self.window_state.scale_factor,
                ));
            }
            WindowEvent::CursorLeft { .. } => {
                self.hovered_widget = None;
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_unified_input(map_pointer_button(
                    button,
                    state,
                    self.window_state.cursor_logical,
                ));
            }
            WindowEvent::Touch(touch) => {
                self.handle_unified_input(map_touch(
                    touch.id,
                    touch.location,
                    touch.phase,
                    self.window_state.scale_factor,
                ));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_unified_input(map_wheel(delta, self.window_state.scale_factor));
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    eprintln!("zode-app: redraw failed: {error}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.drain_accessibility_actions();
        if self.window_state.dirty {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.record_window_geometry();
        self.persist_ui_state();
    }
}

#[cfg(test)]
mod queue_policy_tests {
    use zode_app_model::{
        demo_state, reduce_agent_event, reduce_queue_command, AppCommand, ProjectState,
        ReduceOutcome, TranscriptState,
    };
    use zode_app_ui::{ComposerController, ComposerOutcome, ComposerSubmission, Key, Modifiers};
    use zode_node_protocol::{
        AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
        UserContent, WorkspaceUri, PROTOCOL_VERSION,
    };

    use super::{has_dispatchable_current_session, submission_queue_session};

    fn state_with_session() -> (zode_app_model::ZodeAppState, SessionLocator) {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "session");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace.clone(),
            title: "session".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.current_session = Some(session.clone());
        state.active_workspace = Some(workspace);
        (state, session)
    }

    fn send(text: &str) -> ComposerOutcome {
        ComposerOutcome::Send(ComposerSubmission {
            content: vec![UserContent::Text { text: text.into() }],
            attachments: Vec::new(),
        })
    }

    #[test]
    fn idle_send_joins_an_existing_session_queue_instead_of_bypassing_its_head() {
        let (mut state, session) = state_with_session();
        let head = state
            .message_queues
            .entry(session.clone())
            .or_default()
            .enqueue("existing head".into(), Vec::new())
            .unwrap();

        assert!(has_dispatchable_current_session(&state));
        assert_eq!(
            submission_queue_session(&state, &send("new tail")),
            Some(session.clone())
        );
        let _ = reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session,
                content: vec![UserContent::Text {
                    text: "new tail".into(),
                }],
                attachments: Vec::new(),
            },
        );
        assert_eq!(
            state
                .message_queues
                .values()
                .next()
                .and_then(|queue| queue.items.first())
                .map(|message| message.id),
            Some(head)
        );
        assert_eq!(
            state.message_queues.values().next().map(|queue| queue
                .items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>()),
            Some(vec!["existing head", "new tail"])
        );
    }

    #[test]
    fn idle_send_without_pending_messages_keeps_the_direct_submit_path() {
        let (state, _) = state_with_session();

        assert_eq!(submission_queue_session(&state, &send("direct")), None);
    }

    #[test]
    fn fatal_error_window_still_queues_composer_input_until_turn_finished() {
        let (mut state, session) = state_with_session();
        let turn_id = TurnId::new();
        state.transcripts.get_mut(&session).unwrap().busy = true;
        state.active_turns.insert(session.clone(), turn_id);

        assert_eq!(
            reduce_agent_event(
                &mut state,
                AgentEvent {
                    version: PROTOCOL_VERSION,
                    session: session.clone(),
                    turn_id,
                    sequence: 1,
                    kind: AgentEventKind::Error {
                        message: "provider failed".into(),
                        retryable: false,
                    },
                },
            ),
            ReduceOutcome::Applied,
        );

        let mut composer = ComposerController::new("follow up after failure");
        composer.set_busy(state.transcripts[&session].busy);
        let outcome = composer.key(Key::Enter, Modifiers::NONE);

        assert!(matches!(outcome, ComposerOutcome::Queue(_)));
        assert_eq!(submission_queue_session(&state, &outcome), Some(session));
    }
}
