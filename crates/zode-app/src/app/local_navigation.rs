use zode_app_model::{reduce_navigation_command, AppCommand, NavigationOutcome};

use super::{interaction::normalize_conversation_route, DesktopApp};

impl DesktopApp {
    pub(super) fn apply_local_navigation_command(&mut self, command: &AppCommand) -> bool {
        let previous_session = self.app_state.current_session.clone();
        let previous_queue_edit = self.app_state.composer.editing_queued_message;
        let global_search_was_open = self.app_state.global_search.open;
        let project_picker_was_open = self.app_state.project_picker.open;
        let project_picker_previous_anchor = self.app_state.project_picker.anchor;
        let previous_context_menu = self.app_state.composer.context_menu;
        let previous_footer_menu = self.app_state.composer.footer_menu;
        let outcome = reduce_navigation_command(&mut self.app_state, command.clone());
        let request_open_with_catalog = matches!(command, AppCommand::ToggleOpenWithMenu)
            && self.app_state.open_with.menu_open
            && matches!(
                self.app_state.open_with.applications,
                zode_app_model::LoadState::Loading
            );
        let branch_load_after_toggle = self.branch_load_after_context_toggle(command);
        let open_with_focus = self.sync_open_with_after_navigation(command);
        let session_focus = self.sync_session_action_after_navigation(command);
        self.sync_queue_editor_after_state_change(previous_session.clone(), previous_queue_edit);
        self.prune_queued_payloads();
        let handled = match outcome {
            NavigationOutcome::Applied => {
                let _ = self.persist_local_navigation_effect(command);
                true
            }
            NavigationOutcome::NeedsEffect if matches!(command, AppCommand::CreateProject) => {
                self.request_workspace_pick();
                true
            }
            NavigationOutcome::NeedsEffect
                if matches!(command, AppCommand::OpenProjectInFinder { .. }) =>
            {
                self.apply_project_action_command(command)
            }
            NavigationOutcome::NeedsEffect
                if matches!(command, AppCommand::OpenWorkspaceExternally { .. }) =>
            {
                self.apply_open_with_command(command)
            }
            NavigationOutcome::NeedsEffect
                if matches!(command, AppCommand::LoadExternalApplications) =>
            {
                self.request_open_with_catalog();
                true
            }
            NavigationOutcome::NeedsEffect
                if matches!(
                    command,
                    AppCommand::ToggleProject(_)
                        | AppCommand::SetSessionPinned { .. }
                        | AppCommand::SetSessionArchived { .. }
                        | AppCommand::ArchiveProjectTasks { .. }
                        | AppCommand::RequestDeleteSession(_)
                ) =>
            {
                let _ = self.persist_local_navigation_effect(command);
                true
            }
            outcome
                if self
                    .consume_branch_navigation_outcome(command, outcome)
                    .is_some() =>
            {
                true
            }
            NavigationOutcome::NeedsEffect | NavigationOutcome::Ignored => false,
        };
        if !handled {
            return false;
        }
        normalize_conversation_route(&mut self.app_state, command);
        // Session/task navigation normalizes the conversation chrome directly,
        // outside the presentation reducer. Keep the host-owned animations in
        // lockstep so an outgoing right panel cannot survive the navigation.
        self.sync_primary_sidebar_transition();
        self.sync_right_panel_transition();
        if matches!(
            command,
            AppCommand::BeginTask { .. }
                | AppCommand::SelectSession(_)
                | AppCommand::SetSessionArchived { archived: true, .. }
                | AppCommand::ArchiveProjectTasks { .. }
                | AppCommand::ToggleSidebarTasks
        ) {
            self.persist_ui_state();
        }
        let focus_after = self
            .sync_global_search_after_navigation(command, global_search_was_open)
            .or(open_with_focus)
            .or(session_focus)
            .or_else(|| {
                self.sync_project_picker_after_navigation(
                    command,
                    project_picker_was_open,
                    project_picker_previous_anchor,
                )
            })
            .or_else(|| self.sync_composer_context_after_navigation(command, previous_context_menu))
            .or_else(|| self.sync_composer_footer_after_navigation(command, previous_footer_menu));
        self.refresh_if_session_changed(previous_session);
        self.sync_composer_busy();
        self.rebuild_frame_snapshot();
        if let Some(id) = focus_after {
            self.set_focused_widget(Some(id));
        } else {
            self.request_redraw();
        }
        if let Some(workspace_uri) = branch_load_after_toggle {
            self.enqueue_command(AppCommand::LoadBranches { workspace_uri });
        }
        if request_open_with_catalog {
            self.request_open_with_catalog();
        }
        true
    }
}
