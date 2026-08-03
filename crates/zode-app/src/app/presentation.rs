use std::time::{Duration, Instant};

use zode_app_model::{
    reduce_presentation_command, AppCommand, ChecksState, LoadState, PresentationCommandOutcome,
    PreviewState, PullRequestStatus, SecondaryPane, ShellRoute, ZodeAppState,
};
use zode_app_ui::{
    PointerButton, PointerEvent, PointerEventKind, RectExt, WorkspaceLayout, COMPOSER_ID,
    TERMINAL_ID,
};
use zode_node_protocol::SessionLocator;

use super::DesktopApp;
use crate::{cursor::CursorHint, presentation_bridge::PresentationQuery, window_state::AppWake};

/// How often `maybe_poll_pull_request_status` re-fetches the PR status
/// while it has pending checks - see `PresentationRefresh::PullRequestPoll`.
const PULL_REQUEST_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PresentationRefresh {
    PaneOpened,
    IntegrationsOpened,
    PluginTrustReviewOpened(String),
    /// The detail overlay's "检查更新" was pressed; the reducer already moved
    /// it to `PluginUpdateState::Checking`, this fetches the answer.
    PluginUpdateCheckRequested(String),
    SessionChanged,
    CommandCompleted,
    DiffInvalidated(SessionLocator),
    /// Fired on a fixed cadence by `DesktopApp::maybe_poll_pull_request_status`
    /// only while the current session's PR status has pending checks - see
    /// `docs/proposals/right-panel-parity.md` section 1.1's poll policy. Every
    /// other trigger loads the PR status lazily (once), not on a timer.
    PullRequestPoll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalPresentationOutcome {
    pub refresh: Option<PresentationRefresh>,
}

pub(super) fn reduce_local_presentation_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> Option<LocalPresentationOutcome> {
    let refresh =
        if matches!(&command, AppCommand::Navigate(ShellRoute::Integrations(_)))
            || matches!(&command, AppCommand::SelectIntegrationsTab(_))
                && integrations_need_refresh(state)
        {
            Some(PresentationRefresh::IntegrationsOpened)
        } else if matches!(&command, AppCommand::RequestPluginTrustReview) {
            state.presentation.plugin_detail.as_ref().map(|detail| {
                PresentationRefresh::PluginTrustReviewOpened(detail.plugin_id.clone())
            })
        } else if matches!(&command, AppCommand::CheckPluginUpdate) {
            state.presentation.plugin_detail.as_ref().map(|detail| {
                PresentationRefresh::PluginUpdateCheckRequested(detail.plugin_id.clone())
            })
        } else if matches!(
            &command,
            AppCommand::OpenSecondary(_)
                | AppCommand::OpenReview
                | AppCommand::PreviewWorkspaceFile { .. }
                | AppCommand::SetPinnedSummaryOverlayOpen(true)
                | AppCommand::SetPinnedSummaryAutoHidden(false)
        ) {
            Some(PresentationRefresh::PaneOpened)
        } else {
            None
        };
    if reduce_presentation_command(state, command) == PresentationCommandOutcome::Applied {
        Some(LocalPresentationOutcome { refresh })
    } else {
        None
    }
}

fn integrations_need_refresh(state: &ZodeAppState) -> bool {
    let Some(workspace_uri) = state.active_available_workspace() else {
        return false;
    };
    !matches!(
        &state.presentation.integrations,
        LoadState::Ready(catalog) if &catalog.workspace_uri == workspace_uri
    ) && !matches!(&state.presentation.integrations, LoadState::Loading)
}

pub(super) fn presentation_queries_for_refresh(
    state: &ZodeAppState,
    refresh: PresentationRefresh,
) -> Vec<PresentationQuery> {
    if refresh == PresentationRefresh::IntegrationsOpened {
        let mut queries: Vec<PresentationQuery> = state
            .active_available_workspace()
            .cloned()
            .map(|workspace_uri| PresentationQuery::Integrations { workspace_uri })
            .into_iter()
            .collect();
        queries.push(PresentationQuery::InstalledPlugins);
        return queries;
    }
    if let PresentationRefresh::PluginTrustReviewOpened(plugin_id) = refresh {
        return vec![PresentationQuery::PluginTrustReview { plugin_id }];
    }
    if let PresentationRefresh::PluginUpdateCheckRequested(plugin_id) = refresh {
        return vec![PresentationQuery::PluginUpdateCheck { plugin_id }];
    }
    let Some(session) = state.current_session.as_ref().cloned() else {
        return Vec::new();
    };
    if refresh == PresentationRefresh::PullRequestPoll {
        return state
            .available_workspace_for_session(&session)
            .cloned()
            .map(|workspace_uri| {
                vec![PresentationQuery::PullRequest {
                    session,
                    workspace_uri,
                }]
            })
            .unwrap_or_default();
    }
    if session.session_id.starts_with("local-error-") || !state.transcripts.contains_key(&session) {
        return Vec::new();
    }
    let Some(workspace_uri) = state.available_workspace_for_session(&session).cloned() else {
        return Vec::new();
    };
    let refreshes_session = matches!(
        &refresh,
        PresentationRefresh::PaneOpened
            | PresentationRefresh::SessionChanged
            | PresentationRefresh::CommandCompleted
    ) || matches!(
        &refresh,
        PresentationRefresh::DiffInvalidated(invalidated) if invalidated == &session
    );
    if !refreshes_session {
        return Vec::new();
    }

    let mut queries = Vec::new();
    let summary_requested = state.presentation.route == ShellRoute::Conversation
        && (state.presentation.pinned_summary_overlay_open
            || (!state.presentation.pinned_summary_auto_hidden
                && !state.presentation.secondary_sidebar_open)
            || state.presentation.secondary_pane == Some(SecondaryPane::Environment));
    if summary_requested {
        queries.push(PresentationQuery::Environment {
            session: session.clone(),
            workspace_uri: workspace_uri.clone(),
        });
        queries.push(PresentationQuery::Diff {
            session: session.clone(),
        });
        // Lazy by design (see `PresentationRefresh::PullRequestPoll`'s doc
        // comment): loaded once whenever the RepositoryActions area
        // becomes visible, the session changes, or a manual refresh
        // completes - never eagerly re-polled here.
        queries.push(PresentationQuery::PullRequest {
            session: session.clone(),
            workspace_uri: workspace_uri.clone(),
        });
    }

    match state.presentation.secondary_pane {
        Some(SecondaryPane::Review) => {
            let diff = PresentationQuery::Diff {
                session: session.clone(),
            };
            if !queries.contains(&diff) {
                queries.push(diff);
            }
        }
        Some(SecondaryPane::DocumentPreview)
            if matches!(
                refresh,
                PresentationRefresh::PaneOpened | PresentationRefresh::SessionChanged
            ) =>
        {
            if let Some(query) = state
                .presentation
                .sessions
                .get(&session)
                .and_then(|presentation| presentation.preview.target())
                .filter(|target| target.workspace_uri == workspace_uri)
                .cloned()
                .map(|target| PresentationQuery::DocumentPreview { session, target })
            {
                queries.push(query);
            }
        }
        _ => {}
    }
    queries
}

pub(super) fn mark_presentation_query_failed(
    state: &mut ZodeAppState,
    query: PresentationQuery,
    message: String,
) {
    match query {
        PresentationQuery::Environment { session, .. } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .context = LoadState::Failed(message);
        }
        PresentationQuery::Diff { session } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .diff
                .load = LoadState::Failed(message);
        }
        PresentationQuery::DocumentPreview { session, target } => {
            let preview = &mut state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .preview;
            if preview.target() == Some(&target) {
                *preview = PreviewState::Failed { target, message };
            }
        }
        PresentationQuery::Integrations { .. } => {
            state.presentation.integrations = LoadState::Failed(message);
        }
        PresentationQuery::InstalledPlugins => {
            state.presentation.installed_plugins = LoadState::Failed(message);
        }
        PresentationQuery::PluginTrustReview { plugin_id } => {
            if let Some(detail) = state
                .presentation
                .plugin_detail
                .as_mut()
                .filter(|detail| detail.plugin_id == plugin_id)
            {
                if let zode_app_model::PluginDetailMode::TrustReview { review, .. } =
                    &mut detail.mode
                {
                    *review = LoadState::Failed(message);
                }
            }
        }
        PresentationQuery::PluginUpdateCheck { plugin_id } => {
            if let Some(detail) = state
                .presentation
                .plugin_detail
                .as_mut()
                .filter(|detail| detail.plugin_id == plugin_id)
            {
                detail.update = zode_app_model::PluginUpdateState::CheckFailed(message);
            }
        }
        PresentationQuery::PullRequest { session, .. } => {
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .pull_request
                .load = LoadState::Failed(message);
        }
    }
}

impl DesktopApp {
    pub(super) fn apply_presentation_command(&mut self, command: AppCommand) -> bool {
        // An explicit collapse (button click or the Cmd+B shortcut both land
        // here) doesn't move the toggle button, so the pointer commonly sits
        // right on top of the collapsed chrome's hover-preview trigger the
        // instant the sidebar closes. Arm the suppression latch before the
        // state flips so the next hover check doesn't slide the preview back
        // in over the sidebar that was just dismissed.
        if command == AppCommand::TogglePrimarySidebar && self.app_state.shell.sidebar_open {
            self.arm_primary_sidebar_preview_suppression();
        }
        let settings_category = match &command {
            AppCommand::Navigate(ShellRoute::Settings(category))
            | AppCommand::SelectSettingsCategory(category) => Some(*category),
            _ => None,
        };
        let toggles_retained_terminal = command == AppCommand::ToggleSidebar
            && self.app_state.presentation.secondary_pane == Some(SecondaryPane::Terminal);
        let opens_terminal = command == AppCommand::OpenSecondary(SecondaryPane::Terminal)
            || (toggles_retained_terminal && !self.app_state.presentation.secondary_sidebar_open);
        let replaces_terminal = matches!(
            &command,
            AppCommand::OpenSecondary(pane) if *pane != SecondaryPane::Environment
        );
        let closes_terminal = ((command == AppCommand::CloseSecondary || replaces_terminal)
            && self.app_state.presentation.secondary_pane == Some(SecondaryPane::Terminal)
            && !opens_terminal)
            || (toggles_retained_terminal && self.app_state.presentation.secondary_sidebar_open);
        // Unlike the terminal (which keeps its PTY running while hidden),
        // the browser panel stops streaming the instant it's hidden - CPU
        // discipline the M1 design calls out explicitly (screencast keeps
        // Chrome compositing and encoding for as long as it's active).
        let toggles_retained_browser = command == AppCommand::ToggleSidebar
            && self.app_state.presentation.secondary_pane == Some(SecondaryPane::Browser);
        let opens_browser = command == AppCommand::OpenSecondary(SecondaryPane::Browser)
            || (toggles_retained_browser && !self.app_state.presentation.secondary_sidebar_open);
        let closes_browser = ((command == AppCommand::CloseSecondary || replaces_terminal)
            && self.app_state.presentation.secondary_pane == Some(SecondaryPane::Browser)
            && !opens_browser)
            || (toggles_retained_browser && self.app_state.presentation.secondary_sidebar_open);
        let persist_preferences = matches!(
            command,
            AppCommand::TogglePrimarySidebar
                | AppCommand::SetPrimarySidebarWidth(_)
                | AppCommand::SetSecondarySidebarWidth(_)
        );
        let Some(outcome) = reduce_local_presentation_command(&mut self.app_state, command) else {
            return false;
        };
        self.sync_primary_sidebar_transition();
        self.sync_right_panel_transition();
        if let Some(category) = settings_category {
            self.refresh_local_settings_for_category(category);
        }
        if let Some(refresh) = outcome.refresh {
            self.request_presentation_refresh(refresh);
        }
        if persist_preferences {
            self.persist_ui_state();
        }
        if opens_terminal {
            self.ensure_terminal_runtime();
        }
        if opens_browser {
            self.ensure_browser_runtime();
        }
        if closes_browser {
            self.stop_browser_runtime();
        }
        self.rebuild_frame_snapshot();
        if opens_terminal {
            self.resize_terminal_grid();
            self.set_focused_widget(Some(TERMINAL_ID));
        } else if closes_terminal {
            self.set_focused_widget(Some(COMPOSER_ID));
        } else if opens_browser {
            self.resize_browser_frame_stream();
            self.request_redraw();
        } else if closes_browser {
            self.set_focused_widget(Some(COMPOSER_ID));
        } else {
            self.request_redraw();
        }
        true
    }

    pub(super) fn handle_secondary_sidebar_resize_pointer(&mut self, event: PointerEvent) -> bool {
        let intent = secondary_sidebar_resize_intent(
            self.window_state.secondary_sidebar_resize_active,
            event,
            self.frame_snapshot.layout,
        );
        match intent {
            SecondarySidebarResizeIntent::Ignored => false,
            SecondarySidebarResizeIntent::Hover => {
                self.set_secondary_sidebar_resize_cursor(true);
                true
            }
            SecondarySidebarResizeIntent::Begin => {
                self.window_state.secondary_sidebar_resize_active = true;
                self.set_secondary_sidebar_resize_cursor(true);
                true
            }
            SecondarySidebarResizeIntent::Resize(width) => {
                if reduce_presentation_command(
                    &mut self.app_state,
                    AppCommand::SetSecondarySidebarWidth(width),
                ) == PresentationCommandOutcome::Applied
                {
                    self.rebuild_frame_snapshot();
                    self.request_redraw();
                }
                self.set_secondary_sidebar_resize_cursor(true);
                true
            }
            SecondarySidebarResizeIntent::Finish => {
                self.window_state.secondary_sidebar_resize_active = false;
                self.persist_ui_state();
                let still_over_handle = secondary_sidebar_resize_handle(self.frame_snapshot.layout)
                    .is_some_and(|rect| rect.contains(event.position));
                self.set_secondary_sidebar_resize_cursor(still_over_handle);
                true
            }
        }
    }

    fn set_secondary_sidebar_resize_cursor(&mut self, resize: bool) {
        self.update_native_cursor(if resize {
            CursorHint::ResizeEw
        } else {
            CursorHint::Default
        });
    }

    pub(super) fn cancel_secondary_sidebar_resize(&mut self) {
        if finish_secondary_sidebar_resize(&mut self.window_state.secondary_sidebar_resize_active) {
            self.persist_ui_state();
        }
        self.set_secondary_sidebar_resize_cursor(false);
    }

    pub(super) fn request_presentation_refresh(&mut self, refresh: PresentationRefresh) {
        let queries = presentation_queries_for_refresh(&self.app_state, refresh);
        if queries.is_empty() {
            return;
        }
        let Some(bridge) = self.presentation_queries.as_mut() else {
            for query in queries {
                mark_presentation_query_failed(
                    &mut self.app_state,
                    query,
                    "the presentation query service is unavailable".into(),
                );
            }
            return;
        };
        for query in queries {
            if let Err(query) = bridge.request(&mut self.app_state, query) {
                mark_presentation_query_failed(
                    &mut self.app_state,
                    query,
                    "the presentation query pump is unavailable".into(),
                );
            }
        }
    }

    pub(super) fn drain_presentation_queries(&mut self) -> usize {
        self.presentation_queries
            .as_mut()
            .map_or(0, |queries| queries.drain_into(&mut self.app_state))
    }

    pub(super) fn refresh_if_session_changed(&mut self, previous: Option<SessionLocator>) {
        if previous != self.app_state.current_session {
            self.sync_right_panel_transition();
            self.request_presentation_refresh(PresentationRefresh::SessionChanged);
        }
    }

    /// Re-polls the current session's `gh` pull-request status on a fixed
    /// cadence while it has pending checks; a no-op otherwise. Called from
    /// `redraw` every frame, but throttled by `pull_request_next_poll_at`
    /// so it only actually dispatches a query once per
    /// `PULL_REQUEST_POLL_INTERVAL` - see
    /// `docs/proposals/right-panel-parity.md` section 1.1's poll policy.
    pub(super) fn maybe_poll_pull_request_status(&mut self, now: Instant) {
        let has_pending_checks = self
            .app_state
            .current_session_presentation()
            .and_then(|presentation| presentation.pull_request.load.ready())
            .is_some_and(|status| {
                matches!(
                    status,
                    PullRequestStatus::Pr {
                        checks: ChecksState::Pending,
                        ..
                    }
                )
            });
        if !has_pending_checks {
            self.pull_request_next_poll_at = None;
            return;
        }
        self.start_pull_request_poll_task();
        if self
            .pull_request_next_poll_at
            .is_some_and(|deadline| now < deadline)
        {
            return;
        }
        self.pull_request_next_poll_at = Some(now + PULL_REQUEST_POLL_INTERVAL);
        self.request_presentation_refresh(PresentationRefresh::PullRequestPoll);
    }

    /// Background ticker mirroring `start_browser_wake_task`: wakes the
    /// event loop at a fixed cadence so `maybe_poll_pull_request_status`
    /// actually gets a chance to run between otherwise-unrelated events.
    /// Started once and left running, matching the browser task's own
    /// simplicity - the poll itself is a no-op once checks stop pending.
    fn start_pull_request_poll_task(&mut self) {
        if self.pull_request_poll_task.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        self.pull_request_poll_task = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(PULL_REQUEST_POLL_INTERVAL);
            loop {
                interval.tick().await;
                if proxy.send_event(AppWake::Redraw).is_err() {
                    return;
                }
            }
        }));
    }
}

const SECONDARY_SIDEBAR_RESIZE_HIT_W: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecondarySidebarResizeIntent {
    Ignored,
    Hover,
    Begin,
    Resize(u16),
    Finish,
}

fn secondary_sidebar_resize_intent(
    active: bool,
    event: PointerEvent,
    layout: WorkspaceLayout,
) -> SecondarySidebarResizeIntent {
    if active {
        return match (event.kind, event.button) {
            (PointerEventKind::Move, _) => SecondarySidebarResizeIntent::Resize(
                secondary_sidebar_width_at_pointer(layout, event.position.x),
            ),
            (PointerEventKind::Release, Some(PointerButton::Primary)) => {
                SecondarySidebarResizeIntent::Finish
            }
            _ => SecondarySidebarResizeIntent::Ignored,
        };
    }
    let over_handle = secondary_sidebar_resize_handle(layout)
        .is_some_and(|handle| handle.contains(event.position));
    match (event.kind, event.button, over_handle) {
        (PointerEventKind::Move, _, true) => SecondarySidebarResizeIntent::Hover,
        (PointerEventKind::Press, Some(PointerButton::Primary), true) => {
            SecondarySidebarResizeIntent::Begin
        }
        _ => SecondarySidebarResizeIntent::Ignored,
    }
}

fn secondary_sidebar_resize_handle(layout: WorkspaceLayout) -> Option<jian_widgets::Rect> {
    (layout.review_panel.size.x > 0.0 && layout.divider.size.y > 0.0).then(|| {
        jian_widgets::Rect::xywh(
            layout.divider.origin.x
                - (SECONDARY_SIDEBAR_RESIZE_HIT_W - layout.divider.size.x) / 2.0,
            layout.divider.origin.y,
            SECONDARY_SIDEBAR_RESIZE_HIT_W,
            layout.divider.size.y,
        )
    })
}

fn secondary_sidebar_width_at_pointer(layout: WorkspaceLayout, pointer_x: f32) -> u16 {
    let width = (layout.review_panel.max_x() - pointer_x).round();
    width.clamp(0.0, f32::from(u16::MAX)) as u16
}

fn finish_secondary_sidebar_resize(active: &mut bool) -> bool {
    std::mem::replace(active, false)
}

#[cfg(test)]
mod tests {
    use zode_app_model::{
        demo_state, AppCommand, IntegrationsTab, LoadState, PreviewState, PreviewTarget,
        ProjectState, SecondaryPane, SettingsCategory, ShellRoute, TranscriptState,
    };
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::{
        finish_secondary_sidebar_resize, mark_presentation_query_failed,
        presentation_queries_for_refresh, reduce_local_presentation_command, PresentationRefresh,
    };
    use crate::presentation_bridge::PresentationQuery;

    fn state_with_session(available: bool) -> (zode_app_model::ZodeAppState, SessionLocator) {
        let mut state = demo_state();
        let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "session-1");
        state.projects.push(ProjectState {
            workspace_uri: workspace_uri.clone(),
            expanded: true,
            available,
            last_opened_ms: 0,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace_uri.clone(),
            title: "session".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.current_session = Some(session.clone());
        state.active_workspace = Some(workspace_uri);
        (state, session)
    }

    #[test]
    fn presentation_commands_are_reduced_locally() {
        let commands = [
            AppCommand::Navigate(ShellRoute::Conversation),
            AppCommand::OpenSecondary(SecondaryPane::Environment),
            AppCommand::CloseSecondary,
            AppCommand::SelectSettingsCategory(SettingsCategory::Appearance),
            AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
            AppCommand::OpenReview,
        ];

        for command in commands {
            let mut state = demo_state();
            assert!(reduce_local_presentation_command(&mut state, command).is_some());
        }
        assert!(reduce_local_presentation_command(
            &mut demo_state(),
            AppCommand::SetModel("model".into()),
        )
        .is_none());
    }

    #[test]
    fn cursor_exit_or_focus_loss_clears_the_secondary_drag_latch() {
        let mut active = true;
        assert!(finish_secondary_sidebar_resize(&mut active));
        assert!(!active);
        assert!(!finish_secondary_sidebar_resize(&mut active));
    }

    #[test]
    fn opening_environment_queries_environment_and_canonical_diff() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(
            &mut state,
            AppCommand::OpenSecondary(SecondaryPane::Environment),
        );

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::PaneOpened),
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::Diff {
                    session: session.clone(),
                },
                PresentationQuery::PullRequest {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
            ]
        );
    }

    #[test]
    fn automatic_summary_prefetches_current_session_context() {
        let (state, session) = state_with_session(true);

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::SessionChanged),
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::Diff {
                    session: session.clone(),
                },
                PresentationQuery::PullRequest {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
            ]
        );
    }

    #[test]
    fn opening_review_queries_the_current_real_session() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::PaneOpened),
            vec![PresentationQuery::Diff { session }]
        );
    }

    #[test]
    fn opening_integrations_queries_the_active_workspace_without_session_assembly() {
        let (mut state, _) = state_with_session(true);
        let outcome = reduce_local_presentation_command(
            &mut state,
            AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
        )
        .unwrap();

        assert_eq!(
            outcome.refresh,
            Some(PresentationRefresh::IntegrationsOpened)
        );
        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::IntegrationsOpened),
            vec![
                PresentationQuery::Integrations {
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::InstalledPlugins,
            ]
        );
    }

    #[test]
    fn switching_integration_tabs_reuses_the_current_workspace_catalog() {
        let (mut state, _) = state_with_session(true);
        state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Plugins);
        state.presentation.integrations = LoadState::Ready(zode_app_model::IntegrationCatalog {
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            installed: Vec::new(),
            sections: Vec::new(),
            directory_error: None,
        });

        let outcome = reduce_local_presentation_command(
            &mut state,
            AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
        )
        .unwrap();

        assert_eq!(outcome.refresh, None);
        assert_eq!(
            state.presentation.route,
            ShellRoute::Integrations(IntegrationsTab::Skills)
        );
        assert!(state.presentation.integrations.ready().is_some());
    }

    #[test]
    fn unavailable_or_synthetic_sessions_never_produce_queries() {
        let (mut unavailable, _) = state_with_session(false);
        reduce_local_presentation_command(&mut unavailable, AppCommand::OpenReview);
        assert_eq!(
            presentation_queries_for_refresh(&unavailable, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        let (mut synthetic, _) = state_with_session(true);
        synthetic.current_session = Some(SessionLocator::new(
            synthetic.host.node_id,
            "local-error-example",
        ));
        assert_eq!(
            presentation_queries_for_refresh(&synthetic, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        synthetic.current_session = None;
        assert_eq!(
            presentation_queries_for_refresh(&synthetic, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );

        let (mut incomplete, session) = state_with_session(true);
        incomplete.transcripts.remove(&session);
        reduce_local_presentation_command(&mut incomplete, AppCommand::OpenReview);
        assert_eq!(
            presentation_queries_for_refresh(&incomplete, PresentationRefresh::PaneOpened),
            Vec::<PresentationQuery>::new()
        );
    }

    #[test]
    fn diff_invalidation_refreshes_only_the_matching_open_review() {
        let (mut state, session) = state_with_session(true);
        state.presentation.pinned_summary_auto_hidden = true;
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);
        let other = SessionLocator::new(state.host.node_id, "other");

        assert_eq!(
            presentation_queries_for_refresh(&state, PresentationRefresh::DiffInvalidated(other),),
            Vec::<PresentationQuery>::new()
        );
        assert_eq!(
            presentation_queries_for_refresh(
                &state,
                PresentationRefresh::DiffInvalidated(session.clone()),
            ),
            vec![PresentationQuery::Diff { session }]
        );
    }

    #[test]
    fn matching_diff_invalidation_refreshes_open_environment_and_diff() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(
            &mut state,
            AppCommand::OpenSecondary(SecondaryPane::Environment),
        );
        let other = SessionLocator::new(state.host.node_id, "other");

        assert!(presentation_queries_for_refresh(
            &state,
            PresentationRefresh::DiffInvalidated(other),
        )
        .is_empty());
        assert_eq!(
            presentation_queries_for_refresh(
                &state,
                PresentationRefresh::DiffInvalidated(session.clone()),
            ),
            vec![
                PresentationQuery::Environment {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
                PresentationQuery::Diff {
                    session: session.clone(),
                },
                PresentationQuery::PullRequest {
                    session: session.clone(),
                    workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                },
            ]
        );
    }

    #[test]
    fn query_failure_is_projected_and_not_replanned_without_a_trigger() {
        let (mut state, session) = state_with_session(true);
        reduce_local_presentation_command(&mut state, AppCommand::OpenReview);
        let query = PresentationQuery::Diff {
            session: session.clone(),
        };

        mark_presentation_query_failed(&mut state, query, "query pump stopped".into());

        assert_eq!(
            state.presentation.sessions[&session].diff.load,
            LoadState::Failed("query pump stopped".into())
        );
    }

    #[test]
    fn retargeted_workspace_never_reloads_the_previous_document() {
        let (mut state, session) = state_with_session(true);
        state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default()
            .preview = PreviewState::Ready {
            target: PreviewTarget {
                workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
                relative_path: "docs/old.md".into(),
            },
            title: "old.md".into(),
            content: "old workspace content".into(),
            kind: zode_app_model::PreviewKind::Markdown,
        };
        let new_workspace = WorkspaceUri::new("file:///repo/new").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: new_workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state
            .threads
            .iter_mut()
            .find(|thread| thread.session == session)
            .unwrap()
            .workspace_uri = new_workspace.clone();
        state.active_workspace = Some(new_workspace);

        let queries = presentation_queries_for_refresh(&state, PresentationRefresh::SessionChanged);
        assert!(
            !queries
                .iter()
                .any(|query| matches!(query, PresentationQuery::DocumentPreview { .. })),
            "retargeting planned a stale document query: {queries:?}"
        );
    }
}
