use std::{collections::BTreeSet, sync::Arc, time::Instant};

use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::{ProjectState, ZodeAppState};
use zode_app_ui::{
    Composer, ComposerController, GlobalSearchController, Insets, ProjectPickerController,
    SessionRenameController, TerminalGrid, TerminalPanelController, WidgetId, WorkspaceSnapshot,
};

use crate::services::{
    ExternalApplicationService, ExternalOpenService, LocalExternalApplicationService,
    LocalExternalOpenService, LocalRepositoryService, NativeSessionWindowService,
    RepositoryService, SessionWindowService, WorkspaceService,
};
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    clipboard::{ClipboardService, NativeClipboardService},
    command_bridge::CommandBridge,
    event_bridge::AgentEventBridge,
    event_map::{
        composer_outcome_command, map_ime_input_owned, map_keyboard, map_pointer_button,
        map_pointer_move, map_touch, map_wheel,
    },
    presentation_bridge::PresentationQueryBridge,
    presenter::LivePresenter,
    render::{NativeBackend, RasterSurface},
    services::LocalTerminalService,
    terminal_runtime::TerminalRuntime,
    window_state::{
        AppWake, WindowGeometry, WindowState, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
};
use zode_app_runtime::{workspace_uri_to_path, AppStateStore, TaskContext};
use zode_node_protocol::{AgentEndpoint, NodeCapability, UserContent, WorkspaceUri};

#[path = "app/branch_catalog.rs"]
mod branch_catalog;
mod browser;
#[path = "app/composer_context.rs"]
mod composer_context;
#[path = "app/composer_footer.rs"]
mod composer_footer;
mod computer_use;
#[path = "app/environment-actions.rs"]
mod environment_actions;
#[path = "app/external-preview.rs"]
mod external_preview;
mod global_search;
mod integrations;
mod interaction;
mod lightbox;
#[path = "app/local_navigation.rs"]
mod local_navigation;
mod navigation_persistence;
mod navigation_state;
#[path = "app/open-with.rs"]
mod open_with;
mod persistence;
mod plugin_market;
mod presentation;
#[path = "app/primary-sidebar-preview.rs"]
mod primary_sidebar_preview;
#[path = "app/primary-sidebar-preview-focus.rs"]
mod primary_sidebar_preview_focus;
#[path = "app/primary-sidebar-preview-host.rs"]
mod primary_sidebar_preview_host;
#[path = "app/primary-sidebar-resize.rs"]
mod primary_sidebar_resize;
#[path = "app/primary-sidebar-transition.rs"]
mod primary_sidebar_transition;
#[path = "app/project-actions.rs"]
mod project_actions;
mod project_picker;
#[path = "app/provider-input.rs"]
mod provider_input;
#[path = "app/provider_models.rs"]
mod provider_models;
#[path = "app/provider-setup.rs"]
mod provider_setup;
mod queue;
mod queue_focus;
#[path = "app/right-panel-transition.rs"]
mod right_panel_transition;
mod session_menu;
mod settings;
mod sidebar;
#[path = "app/sidebar-menu.rs"]
mod sidebar_menu;
#[path = "app/sidebar-motion.rs"]
mod sidebar_motion;
mod startup;
mod terminal;
mod transcript_find;
#[path = "app/transcript-images.rs"]
mod transcript_images;
mod window;

pub use startup::{run_demo, run_demo_for_session};

pub struct DesktopApp {
    app_state: ZodeAppState,
    a11y: Option<AccessibilityHost>,
    window: Option<Arc<Window>>,
    presenter: Option<LivePresenter>,
    raster: Option<RasterSurface>,
    renderer: NativeBackend,
    window_state: WindowState,
    proxy: EventLoopProxy<AppWake>,
    agent_events: Option<AgentEventBridge>,
    composer: ComposerController,
    global_search_controller: GlobalSearchController,
    /// Editable buffer for the in-conversation find bar. A separate
    /// controller from `global_search_controller` even though both are the
    /// same type: the two fields are visible at the same time and must keep
    /// independent carets, selections and IME compositions.
    transcript_find_controller: zode_app_ui::TranscriptFindController,
    project_picker_controller: ProjectPickerController,
    branch_picker_controller: ProjectPickerController,
    session_rename_controller: SessionRenameController,
    provider_models_controller: provider_models::ProviderModelsController,
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
    /// Process-wide browser session shared with the agent's `browser_*`
    /// tools (see `zode_core::engine::EngineTemplate::browser`) - attached
    /// post-construction via `attach_browser_session`, mirroring
    /// `attach_endpoint`. `None` until attached (e.g. in tests that never
    /// call it), in which case the spectator panel stays idle.
    browser_session: Option<Arc<zode_core::browser::BrowserSession>>,
    /// Latest decoded-ready frame handed to the panel's paint call. `None`
    /// before the first frame or once the stream stops.
    browser_frame: Option<browser::BrowserFrameCache>,
    /// Last `ScreencastFrame::sequence` observed - lets `poll_browser_frame`
    /// tell "new frame" from "still the same frame" without touching bytes.
    browser_frame_seq: u64,
    /// Monotonic counter minted fresh for every NEW frame (never reused for
    /// repeat paints of the same frame) - becomes the image cache key so a
    /// stale decode is never shown. See `NativeBackend::image_source`.
    browser_image_id: u64,
    /// Background ticker that wakes the event loop at a fixed cadence while
    /// the panel is visible, so `poll_browser_frame` actually gets called
    /// between otherwise-unrelated events - screencast frames don't arrive
    /// through any existing wake path. Aborted when the panel closes.
    browser_wake_task: Option<tokio::task::JoinHandle<()>>,
    /// Background ticker mirroring `browser_wake_task`, started the first
    /// time a pull request shows pending checks so `redraw` gets a chance
    /// to re-poll `gh` on a fixed cadence - see
    /// `docs/proposals/right-panel-parity.md` section 1.1's poll policy and
    /// `maybe_poll_pull_request_status`. Outside that window the PR status
    /// is lazy (loaded once per pane-open/session-change/manual refresh),
    /// never on a timer, so this never starts.
    pull_request_poll_task: Option<tokio::task::JoinHandle<()>>,
    /// Earliest time `maybe_poll_pull_request_status` may re-request the PR
    /// status; `None` means no poll is currently scheduled.
    pull_request_next_poll_at: Option<Instant>,
    modifiers: ModifiersState,
    command_bridge: Option<CommandBridge>,
    presentation_queries: Option<PresentationQueryBridge>,
    scroll_touch: crate::input_dispatch::ScrollTouchTracker,
    frame_snapshot: WorkspaceSnapshot,
    frame_snapshot_valid: bool,
    accessibility_tree_dirty: bool,
    ime: crate::ime::ImeState,
    cursor: crate::cursor::NativeCursorState,
    terminal_grid_resize_pending: bool,
    window_metrics_update_pending: bool,
    /// Set on every `Resized`/`ScaleFactorChanged` event to `now + settle
    /// duration`; cleared once a redraw observes the deadline has passed.
    /// While set, live-resize ticks skip the expensive AccessKit tree push
    /// and keep the raster surface's current allocation (see
    /// `app/window.rs`).
    resize_settle_deadline: Option<Instant>,
    focused_widget: Option<WidgetId>,
    hovered_widget: Option<WidgetId>,
    primary_sidebar_preview: primary_sidebar_preview::PrimarySidebarPreview,
    primary_sidebar_preview_focus: Option<WidgetId>,
    primary_sidebar_transition: primary_sidebar_transition::PrimarySidebarTransition,
    right_panel_transition: right_panel_transition::RightPanelTransition,
    window_focused: bool,
    clipboard: Option<Arc<dyn ClipboardService>>,
    external_open: Arc<dyn ExternalOpenService>,
    repository_service: Arc<dyn RepositoryService>,
    session_window: Arc<dyn SessionWindowService>,
    workspace_picker: project_picker::WorkspacePickerEffect,
    branch_catalog: branch_catalog::BranchCatalogEffect,
    open_with_effect: open_with::OpenWithEffect,
    /// Loads and caches decoded bytes for `TranscriptItem::Image` items in
    /// the current transcript, feeding the `TranscriptImageSource` passed
    /// into `WorkspaceShell::paint_snapshot_with_overlays` - see
    /// `app/transcript-images.rs`.
    transcript_images: transcript_images::TranscriptImageEffect,
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
            app_state.shell.sidebar_open = app_state.ui_preferences.primary_sidebar_open;
            navigation_state::hydrate_session_navigation(&mut app_state, persisted);
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
            navigation_state::hydrate_project_navigation(&mut app_state, persisted);
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
        let global_search_controller =
            GlobalSearchController::new(app_state.global_search.query.clone());
        let project_picker_controller =
            ProjectPickerController::new(app_state.project_picker.search.clone());
        let branch_picker_controller =
            ProjectPickerController::new(app_state.composer.branch_picker.query.clone());
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
        let branch_catalog = branch_catalog::BranchCatalogEffect::new(proxy.clone());
        let open_with_effect = open_with::OpenWithEffect::new(
            proxy.clone(),
            Arc::new(LocalExternalApplicationService),
        );
        let transcript_images = transcript_images::TranscriptImageEffect::new(proxy.clone());
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
        let primary_sidebar_transition =
            primary_sidebar_transition::PrimarySidebarTransition::seeded(
                app_state.shell.sidebar_open,
            );
        let right_panel_transition = right_panel_transition::RightPanelTransition::seeded(
            right_panel_transition::targets_for_state(&app_state, DEFAULT_WINDOW_WIDTH as f32),
        );
        Self {
            app_state,
            a11y: None,
            window: None,
            presenter: None,
            raster: None,
            renderer: NativeBackend::new(1.0),
            window_state: WindowState::new(1221, 992, 1.0),
            proxy,
            agent_events: None,
            composer,
            global_search_controller,
            transcript_find_controller: zode_app_ui::TranscriptFindController::default(),
            project_picker_controller,
            branch_picker_controller,
            session_rename_controller: SessionRenameController::default(),
            provider_models_controller: provider_models::ProviderModelsController::default(),
            queued_payloads: queue::QueuedPayloadStore::default(),
            provisional_sessions: BTreeSet::new(),
            terminal_grid: TerminalGrid::new(80, 24),
            terminal_controller: TerminalPanelController::default(),
            terminal_runtime,
            terminal_workspace: None,
            browser_session: None,
            browser_frame: None,
            browser_frame_seq: 0,
            browser_image_id: 0,
            browser_wake_task: None,
            pull_request_poll_task: None,
            pull_request_next_poll_at: None,
            modifiers: ModifiersState::empty(),
            command_bridge: None,
            presentation_queries: None,
            scroll_touch: crate::input_dispatch::ScrollTouchTracker::default(),
            frame_snapshot,
            frame_snapshot_valid: true,
            accessibility_tree_dirty: true,
            ime: crate::ime::ImeState::default(),
            cursor: crate::cursor::NativeCursorState::default(),
            terminal_grid_resize_pending: false,
            window_metrics_update_pending: false,
            resize_settle_deadline: None,
            focused_widget,
            hovered_widget: None,
            primary_sidebar_preview: primary_sidebar_preview::PrimarySidebarPreview::default(),
            primary_sidebar_preview_focus: None,
            primary_sidebar_transition,
            right_panel_transition,
            window_focused: false,
            clipboard,
            external_open: Arc::new(LocalExternalOpenService),
            repository_service: Arc::new(LocalRepositoryService),
            session_window: Arc::new(NativeSessionWindowService),
            workspace_picker,
            branch_catalog,
            open_with_effect,
            transcript_images,
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

    pub fn set_repository_service(&mut self, service: Arc<dyn RepositoryService>) {
        self.repository_service = service;
    }

    pub fn set_external_application_service(
        &mut self,
        service: Arc<dyn ExternalApplicationService>,
    ) {
        self.open_with_effect.set_service(service);
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

    /// Attaches the process-wide browser session - the SAME `Arc` the
    /// assembled `ZodeEngine`s hand to `browser_read`/`browser_act`, so the
    /// spectator panel (M1 route A) observes the actual browser the agent
    /// drives rather than a second, unrelated Chrome instance.
    pub fn attach_browser_session(&mut self, session: Arc<zode_core::browser::BrowserSession>) {
        self.browser_session = Some(session);
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

    /// Seeds `text` into the composer draft, replacing whatever was there.
    /// Used by the environment panel's "修复" merge-conflict action (see
    /// `environment_actions::EnvironmentActionOutcome::SeedComposerPrompt`)
    /// to hand a preset fix prompt to the agent without mutating the
    /// repository directly. Goes through the same controller-to-state sync
    /// path as normal typing (`ComposerController::set_text` then
    /// `interaction::sync_draft`) so the composer's own invariants
    /// (composition clearing, etc) stay intact.
    pub(super) fn seed_composer_prompt(&mut self, prompt: String) {
        self.composer.set_text(prompt);
        interaction::sync_draft(&mut self.app_state, self.composer.text());
        self.rebuild_frame_snapshot();
        self.request_redraw();
    }

    fn apply_composer_outcome(&mut self, mut outcome: zode_app_ui::ComposerOutcome) {
        let incremental_edit = matches!(outcome, zode_app_ui::ComposerOutcome::Edited);
        let ignored = matches!(outcome, zode_app_ui::ComposerOutcome::Ignored);
        let can_submit_before =
            incremental_edit.then(|| Composer::can_submit(&self.app_state.composer));
        let draft_changed = interaction::sync_draft(&mut self.app_state, self.composer.text());
        let can_submit_changed =
            composer_submit_capability_changed(can_submit_before, &self.app_state);

        if self.redirect_unconfigured_submission(&outcome) {
            return;
        }

        let editing = self
            .app_state
            .current_session
            .clone()
            .zip(self.app_state.composer.editing_queued_message);
        let edited_submission = match &outcome {
            zode_app_ui::ComposerOutcome::Send(submission)
            | zode_app_ui::ComposerOutcome::Queue(submission)
            | zode_app_ui::ComposerOutcome::QueueFront(submission) => Some(submission),
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
            let front = matches!(outcome, zode_app_ui::ComposerOutcome::QueueFront(_));
            let submission = match &mut outcome {
                zode_app_ui::ComposerOutcome::Send(submission)
                | zode_app_ui::ComposerOutcome::Queue(submission)
                | zode_app_ui::ComposerOutcome::QueueFront(submission) => submission,
                _ => unreachable!("only sendable composer outcomes can enter a message queue"),
            };
            let command = zode_app_model::AppCommand::EnqueueMessage {
                session,
                content: std::mem::take(&mut submission.content),
                attachments: submission.attachments.clone(),
                front,
            };
            // A regular Send can arrive while the session is idle even though
            // it already owns pending queue items. Project it like Queue so it
            // cannot appear in the transcript ahead of the existing head.
            self.app_state.composer.attachments.clear();
            self.enqueue_command(command);
            return;
        }

        if matches!(
            outcome,
            zode_app_ui::ComposerOutcome::Queue(_) | zode_app_ui::ComposerOutcome::QueueFront(_)
        ) {
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
        if ignored {
            return;
        }
        if incremental_edit {
            if can_submit_changed {
                self.rebuild_frame_snapshot();
            } else {
                self.refresh_composer_snapshot_value(draft_changed);
            }
            self.ime.mark_area_dirty();
            self.request_redraw();
            return;
        }
        self.rebuild_frame_snapshot();
        self.request_redraw();
    }
}

fn composer_submit_capability_changed(before: Option<bool>, state: &ZodeAppState) -> bool {
    before.is_some_and(|before| before != Composer::can_submit(&state.composer))
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
        zode_app_ui::ComposerOutcome::Queue(_) | zode_app_ui::ComposerOutcome::QueueFront(_) => {
            Some(session)
        }
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
                let branch_catalogs_applied = self.drain_branch_catalog_results();
                let external_applications_applied = self.drain_open_with_results();
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
                let browser_poll = self.poll_browser_frame();
                let transcript_images_changed = self.poll_transcript_images();
                let background_changed = event_drain.changed
                    || commands_applied > 0
                    || queries_applied > 0
                    || workspace_picks_applied > 0
                    || branch_catalogs_applied > 0
                    || external_applications_applied > 0
                    || terminal_changed
                    || browser_poll.state_changed
                    || transcript_images_changed;
                if background_changed {
                    self.rebuild_frame_snapshot();
                }
                self.sync_composer_busy();
                // A new frame with no other state change (the common case
                // while streaming) skips the snapshot rebuild above - the
                // bitmap lives outside `ZodeAppState` on purpose - but must
                // still redraw, or the panel would only repaint whenever
                // something unrelated also changed.
                if background_changed || browser_poll.new_frame {
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
                self.sync_right_panel_transition_for_viewport_change();
                self.begin_resize_settle();
                self.terminal_grid_resize_pending = true;
                self.window_metrics_update_pending = true;
                self.request_redraw();
            }
            WindowEvent::Moved(_) => {
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_state.scale_factor = scale_factor;
                self.renderer = NativeBackend::new(scale_factor as f32);
                self.sync_right_panel_transition_for_viewport_change();
                self.begin_resize_settle();
                self.terminal_grid_resize_pending = true;
                self.window_metrics_update_pending = true;
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
                let command_was_pressed = self.modifiers.super_key();
                self.modifiers = modifiers.state();
                if command_was_pressed != self.modifiers.super_key() {
                    self.request_redraw();
                }
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
                self.handle_unified_input(map_ime_input_owned(event));
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_unified_input(map_pointer_move(
                    position,
                    self.window_state.scale_factor,
                ));
            }
            WindowEvent::CursorLeft { .. } => {
                self.cancel_primary_sidebar_resize();
                self.cancel_secondary_sidebar_resize();
                self.dismiss_primary_sidebar_preview();
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
#[path = "app/queue-policy-tests.rs"]
mod queue_policy_tests;
