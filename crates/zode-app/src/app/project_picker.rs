use std::sync::Arc;

use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{AppCommand, ProjectPickerAnchor, ProjectState};
use zode_app_ui::{
    ImeEvent, Key, KeyEvent, Modifiers, ProjectPicker, ProjectSearchOutcome, WidgetId, COMPOSER_ID,
    COMPOSER_PROJECT_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_SURFACE_ID, PROJECT_PICKER_TRIGGER_ID,
    SIDEBAR_SEARCH_ID,
};
use zode_node_protocol::{SessionLocator, WorkspaceUri};

use crate::{
    command_projection::now_ms,
    services::{LocalWorkspaceService, WorkspaceService},
    window_state::AppWake,
};

use super::DesktopApp;

/// Owns the asynchronous native folder picker without blocking winit.
pub(super) struct WorkspacePickerEffect {
    service: Arc<dyn WorkspaceService>,
    result_sender: mpsc::UnboundedSender<WorkspacePickOutcome>,
    results: mpsc::UnboundedReceiver<WorkspacePickOutcome>,
    proxy: EventLoopProxy<AppWake>,
    in_flight: bool,
}

struct WorkspacePickOutcome {
    expected_session: Option<SessionLocator>,
    expected_workspace: Option<WorkspaceUri>,
    expected_route: zode_app_model::ShellRoute,
    result: Result<Option<WorkspaceUri>, String>,
}

impl DesktopApp {
    pub(super) fn project_picker_view_state(&self) -> zode_app_ui::ProjectPickerViewState {
        zode_app_ui::ProjectPickerViewState {
            open: self.app_state.project_picker.open,
            query: self.project_picker_controller.text().to_owned(),
        }
    }

    pub(super) fn request_workspace_pick(&mut self) {
        self.workspace_picker.request(
            self.app_state.current_session.clone(),
            self.app_state.active_workspace.clone(),
            self.app_state.presentation.route,
        );
    }

    pub(super) fn sync_project_picker_after_navigation(
        &mut self,
        command: &AppCommand,
        was_open: bool,
        previous_anchor: ProjectPickerAnchor,
    ) -> Option<WidgetId> {
        if !self.app_state.project_picker.open {
            self.project_picker_controller.set_text("");
        }
        match command {
            AppCommand::ToggleProjectPicker
                if self.app_state.project_picker.open
                    && (!was_open || previous_anchor != ProjectPickerAnchor::Welcome) =>
            {
                self.project_picker_controller.set_text("");
                Some(PROJECT_PICKER_SEARCH_ID)
            }
            AppCommand::ToggleComposerProjectPicker
                if self.app_state.project_picker.open
                    && (!was_open || previous_anchor != ProjectPickerAnchor::Composer) =>
            {
                self.project_picker_controller.set_text("");
                Some(PROJECT_PICKER_SEARCH_ID)
            }
            AppCommand::ToggleSidebarProjectPicker
                if self.app_state.project_picker.open
                    && (!was_open || previous_anchor != ProjectPickerAnchor::Sidebar) =>
            {
                self.project_picker_controller.set_text("");
                Some(PROJECT_PICKER_SEARCH_ID)
            }
            AppCommand::ToggleProjectPicker
            | AppCommand::ToggleSidebarProjectPicker
            | AppCommand::CloseProjectPicker => Some(project_picker_trigger(previous_anchor)),
            AppCommand::ToggleComposerProjectPicker => Some(COMPOSER_PROJECT_ID),
            AppCommand::BeginTask { .. } => Some(COMPOSER_ID),
            _ => None,
        }
    }

    pub(super) fn project_picker_contains_point(&self, point: jian_widgets::Point2D) -> bool {
        self.frame_snapshot
            .node(PROJECT_PICKER_SURFACE_ID)
            .is_some_and(|node| node.rect.contains(point))
    }

    pub(super) fn project_picker_allows_accessibility_action(&self, id: WidgetId) -> bool {
        if !self.app_state.project_picker.open {
            return true;
        }
        matches!(
            id,
            PROJECT_PICKER_SEARCH_ID
                | PROJECT_PICKER_NEW_ID
                | PROJECT_PICKER_PROJECTLESS_ID
                | SIDEBAR_SEARCH_ID
        ) || ProjectPicker::choices(&self.app_state, self.project_picker_controller.text())
            .into_iter()
            .take(5)
            .any(|choice| ProjectPicker::project_widget_id(&choice.workspace_uri) == id)
    }

    pub(super) fn close_project_picker_from_outside(&mut self) {
        self.enqueue_command(AppCommand::CloseProjectPicker);
    }

    pub(super) fn handle_project_picker_ime(&mut self, event: ImeEvent) -> bool {
        if !self.app_state.project_picker.open
            || self.focused_widget != Some(PROJECT_PICKER_SEARCH_ID)
        {
            return false;
        }
        if self.project_picker_controller.ime(event) == ProjectSearchOutcome::Edited {
            self.sync_project_search_from_controller();
        }
        true
    }

    pub(super) fn handle_project_picker_key(&mut self, event: &KeyEvent) -> bool {
        if !self.app_state.project_picker.open || !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::CloseProjectPicker);
            return true;
        }
        let focus_ids = self.project_picker_focus_ids();
        if event.key == Key::Tab {
            let backwards = event.modifiers.contains(Modifiers::SHIFT);
            self.cycle_project_picker_focus(&focus_ids, backwards);
            return true;
        }
        if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            self.cycle_project_picker_focus(&focus_ids, event.key == Key::ArrowUp);
            return true;
        }
        if self.focused_widget == Some(PROJECT_PICKER_SEARCH_ID) {
            if event.key == Key::Enter
                && self
                    .project_picker_controller
                    .input_state()
                    .composition()
                    .is_some()
            {
                let _ = self
                    .project_picker_controller
                    .key(event.key.clone(), event.modifiers);
                self.sync_project_search_from_controller();
                return true;
            }
            if event.key == Key::Enter {
                if let Some(id) = focus_ids.get(1).copied() {
                    self.activate_widget(id);
                }
                return true;
            }
            if self
                .project_picker_controller
                .key(event.key.clone(), event.modifiers)
                == ProjectSearchOutcome::Edited
            {
                self.sync_project_search_from_controller();
                return true;
            }
            let is_paste = event.modifiers.primary()
                && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"));
            return !is_paste;
        }
        event.modifiers.primary()
    }

    pub(super) fn set_project_search_value(&mut self, value: String) {
        self.project_picker_controller.set_text(value);
        self.sync_project_search_from_controller();
    }

    pub(super) fn paste_project_search_text(&mut self, text: &str) -> bool {
        if !self.app_state.project_picker.open
            || self.focused_widget != Some(PROJECT_PICKER_SEARCH_ID)
        {
            return false;
        }
        if self.project_picker_controller.paste_text(text) == ProjectSearchOutcome::Edited {
            self.sync_project_search_from_controller();
        }
        true
    }

    fn sync_project_search_from_controller(&mut self) {
        self.enqueue_command(AppCommand::SetProjectSearch(
            self.project_picker_controller.text().to_owned(),
        ));
    }

    fn project_picker_focus_ids(&self) -> Vec<WidgetId> {
        let mut ids = vec![PROJECT_PICKER_SEARCH_ID];
        ids.extend(
            ProjectPicker::choices(&self.app_state, self.project_picker_controller.text())
                .into_iter()
                .take(5)
                .map(|choice| ProjectPicker::project_widget_id(&choice.workspace_uri)),
        );
        ids.extend([PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID]);
        ids.retain(|id| self.frame_snapshot.node(*id).is_some());
        ids
    }

    fn cycle_project_picker_focus(&mut self, ids: &[WidgetId], backwards: bool) {
        if ids.is_empty() {
            return;
        }
        let current = self
            .focused_widget
            .and_then(|focused| ids.iter().position(|id| *id == focused));
        let index = match (backwards, current) {
            (false, Some(index)) => (index + 1) % ids.len(),
            (true, Some(0)) => ids.len() - 1,
            (true, Some(index)) => index - 1,
            (false, None) => 0,
            (true, None) => ids.len() - 1,
        };
        self.enqueue_command(AppCommand::SetProjectPickerActive(index.saturating_sub(1)));
        self.set_focused_widget(Some(ids[index]));
    }

    pub(super) fn drain_workspace_pick_results(&mut self) -> usize {
        let results = self.workspace_picker.drain();
        let mut applied = 0;
        for outcome in results {
            if !workspace_pick_matches(
                &self.app_state,
                &outcome.expected_session,
                &outcome.expected_workspace,
                outcome.expected_route,
            ) {
                continue;
            }
            match outcome.result {
                Ok(Some(workspace_uri)) => {
                    if let Some(project) = self
                        .app_state
                        .projects
                        .iter_mut()
                        .find(|project| project.workspace_uri == workspace_uri)
                    {
                        project.available = true;
                        project.expanded = true;
                        project.last_opened_ms = now_ms();
                    } else {
                        self.app_state.projects.push(ProjectState {
                            workspace_uri: workspace_uri.clone(),
                            expanded: true,
                            available: true,
                            last_opened_ms: now_ms(),
                        });
                    }
                    self.enqueue_command(AppCommand::BeginTask {
                        workspace_uri: Some(workspace_uri),
                    });
                    applied += 1;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("zode-app: project folder could not be selected: {error}");
                }
            }
        }
        applied
    }
}

fn project_picker_trigger(anchor: ProjectPickerAnchor) -> WidgetId {
    match anchor {
        ProjectPickerAnchor::Welcome => PROJECT_PICKER_TRIGGER_ID,
        ProjectPickerAnchor::Composer => COMPOSER_PROJECT_ID,
        ProjectPickerAnchor::Sidebar => SIDEBAR_SEARCH_ID,
    }
}

fn workspace_pick_matches(
    state: &zode_app_model::ZodeAppState,
    expected_session: &Option<SessionLocator>,
    expected_workspace: &Option<WorkspaceUri>,
    expected_route: zode_app_model::ShellRoute,
) -> bool {
    &state.current_session == expected_session
        && &state.active_workspace == expected_workspace
        && state.presentation.route == expected_route
}

impl WorkspacePickerEffect {
    pub(super) fn new(proxy: EventLoopProxy<AppWake>) -> Self {
        let (result_sender, results) = mpsc::unbounded_channel();
        Self {
            service: Arc::new(LocalWorkspaceService),
            result_sender,
            results,
            proxy,
            in_flight: false,
        }
    }

    pub(super) fn set_service(&mut self, service: Arc<dyn WorkspaceService>) {
        self.service = service;
    }

    pub(super) fn request(
        &mut self,
        expected_session: Option<SessionLocator>,
        expected_workspace: Option<WorkspaceUri>,
        expected_route: zode_app_model::ShellRoute,
    ) {
        if self.in_flight {
            return;
        }
        let service = self.service.clone();
        let sender = self.result_sender.clone();
        let proxy = self.proxy.clone();
        self.in_flight = true;
        tokio::spawn(async move {
            let result = service
                .pick_workspace()
                .await
                .map_err(|error| error.to_string());
            if sender
                .send(WorkspacePickOutcome {
                    expected_session,
                    expected_workspace,
                    expected_route,
                    result,
                })
                .is_ok()
            {
                let _ = proxy.send_event(AppWake::Redraw);
            }
        });
    }

    fn drain(&mut self) -> Vec<WorkspacePickOutcome> {
        let mut drained = Vec::new();
        while let Ok(result) = self.results.try_recv() {
            self.in_flight = false;
            drained.push(result);
        }
        drained
    }
}

#[cfg(test)]
mod tests {
    use super::workspace_pick_matches;
    use zode_app_model::demo_state;
    use zode_node_protocol::WorkspaceUri;

    #[test]
    fn native_picker_result_is_stale_after_task_context_changes() {
        let mut state = demo_state();
        let original = WorkspaceUri::new("file:///repo/original").unwrap();
        let newer = WorkspaceUri::new("file:///repo/newer").unwrap();
        state.active_workspace = Some(original.clone());
        assert!(workspace_pick_matches(
            &state,
            &None,
            &Some(original.clone()),
            state.presentation.route,
        ));

        state.active_workspace = Some(newer);
        assert!(!workspace_pick_matches(
            &state,
            &None,
            &Some(original),
            state.presentation.route,
        ));
    }
}
