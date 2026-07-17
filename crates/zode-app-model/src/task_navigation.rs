use crate::{
    AppCommand, BranchCatalogState, LoadState, NavigationOutcome, ProjectPickerAnchor,
    ProjectPickerState, ShellPage, ShellRoute, TaskLaunchMode, ZodeAppState,
};

/// Applies new-task and project-picker commands without touching live sessions.
pub(crate) fn reduce_task_navigation(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<NavigationOutcome> {
    match command {
        AppCommand::BeginTask { workspace_uri } => {
            if workspace_uri
                .as_ref()
                .is_some_and(|workspace| !state.available_workspace(workspace))
            {
                return Some(NavigationOutcome::Ignored);
            }
            let context_changed = state.active_workspace.as_ref() != workspace_uri.as_ref();
            if let Some(workspace) = workspace_uri {
                let newest = state
                    .projects
                    .iter()
                    .map(|project| project.last_opened_ms)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                if let Some(project) = state
                    .projects
                    .iter_mut()
                    .find(|project| &project.workspace_uri == workspace)
                {
                    project.last_opened_ms = newest;
                }
            }
            state.current_session = None;
            state.close_session_action_surfaces();
            state.active_workspace.clone_from(workspace_uri);
            state.composer.queue_menu = None;
            state.composer.finish_queue_edit();
            close_project_picker(state);
            reset_composer_task_context(state);
            state.presentation.route = ShellRoute::Conversation;
            state.presentation.secondary_pane = None;
            if context_changed {
                state.presentation.integrations = LoadState::Idle;
            }
            state.review.open = false;
            state.shell.page = ShellPage::Conversation;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ToggleProjectPicker => {
            toggle_project_picker(state, ProjectPickerAnchor::Welcome);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ToggleComposerProjectPicker => {
            toggle_project_picker(state, ProjectPickerAnchor::Composer);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::CloseProjectPicker => {
            close_project_picker(state);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetProjectSearch(search) => {
            state.project_picker.search.clone_from(search);
            state.project_picker.active_index = 0;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetProjectPickerActive(index) => {
            state.project_picker.active_index = *index;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ToggleComposerContextMenu(menu) => {
            close_project_picker(state);
            state.composer.footer_menu = None;
            state.composer.context_menu =
                (state.composer.context_menu != Some(*menu)).then_some(*menu);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::CloseComposerContextMenu => {
            state.composer.context_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ToggleComposerFooterMenu(menu) => {
            close_project_picker(state);
            state.composer.context_menu = None;
            state.composer.footer_menu =
                (state.composer.footer_menu != Some(*menu)).then_some(*menu);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::CloseComposerFooterMenu => {
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetModel(model) if state.current_session.is_none() => {
            if !state
                .composer
                .available_models
                .iter()
                .any(|item| item == model)
            {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.model = Some(model.clone());
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetEffort(effort) if state.current_session.is_none() => {
            if !matches!(effort.as_str(), "low" | "medium" | "high" | "xhigh") {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.effort = Some(effort.clone());
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetSandbox { mode, network } if state.current_session.is_none() => {
            state.composer.sandbox_mode = *mode;
            state.composer.sandbox_network = *network;
            state.composer.sandbox_label = sandbox_label(*mode, *network).into();
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetPermissionPreset {
            approval_mode,
            sandbox_mode,
            network,
        } if state.current_session.is_none() => {
            state.composer.approval_mode = *approval_mode;
            state.composer.sandbox_mode = *sandbox_mode;
            state.composer.sandbox_network = *network;
            state.composer.sandbox_label = approval_label(*approval_mode).into();
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::ResetComposerRuntime if state.current_session.is_none() => {
            let Some(defaults) = state.composer_defaults.clone() else {
                return Some(NavigationOutcome::Ignored);
            };
            state.composer.model = defaults.active_model;
            state.composer.effort = defaults.effort;
            state.composer.approval_mode = defaults.approval_mode;
            state.composer.sandbox_mode = defaults.sandbox_mode;
            state.composer.sandbox_network = defaults.sandbox_network;
            state.composer.sandbox_label = approval_label(defaults.approval_mode).into();
            state.composer.footer_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SelectTaskLaunchMode(mode) => {
            if *mode == TaskLaunchMode::Worktree {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.launch_mode = *mode;
            state.composer.context_menu = None;
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SetBranchSearch(query) => {
            state.composer.branch_picker.query.clone_from(query);
            Some(NavigationOutcome::Applied)
        }
        AppCommand::LoadBranches { workspace_uri } => {
            if state.active_available_workspace() != Some(workspace_uri) {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.branch_picker.catalog = BranchCatalogState::Loading {
                workspace_uri: workspace_uri.clone(),
            };
            Some(NavigationOutcome::NeedsEffect)
        }
        AppCommand::BranchesLoaded(catalog) => {
            let loading_matches = matches!(
                &state.composer.branch_picker.catalog,
                BranchCatalogState::Loading { workspace_uri }
                    if workspace_uri == &catalog.workspace_uri
            );
            let switching_matches = matches!(
                &state.composer.branch_picker.catalog,
                BranchCatalogState::Switching {
                    workspace_uri,
                    branch,
                    ..
                } if workspace_uri == &catalog.workspace_uri && branch == &catalog.current
            );
            if !(loading_matches || switching_matches)
                || state.active_available_workspace() != Some(&catalog.workspace_uri)
            {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.selected_branch = Some(catalog.current.clone());
            if switching_matches {
                state.composer.context_menu = None;
            }
            state.composer.branch_picker.catalog = BranchCatalogState::Ready(catalog.clone());
            Some(NavigationOutcome::Applied)
        }
        AppCommand::BranchesFailed {
            workspace_uri,
            message,
        } => {
            let loading_matches = matches!(
                &state.composer.branch_picker.catalog,
                BranchCatalogState::Loading { workspace_uri: loading }
                    if loading == workspace_uri
            );
            let switching_matches = matches!(
                &state.composer.branch_picker.catalog,
                BranchCatalogState::Switching {
                    workspace_uri: switching,
                    ..
                } if switching == workspace_uri
            );
            if !(loading_matches || switching_matches)
                || state.active_available_workspace() != Some(workspace_uri)
            {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.branch_picker.catalog = BranchCatalogState::Failed {
                workspace_uri: workspace_uri.clone(),
                message: message.clone(),
            };
            Some(NavigationOutcome::Applied)
        }
        AppCommand::SelectBranch {
            workspace_uri,
            branch,
        } => {
            let (catalog_workspace, current, branch_exists, dirty_files) =
                match &state.composer.branch_picker.catalog {
                    BranchCatalogState::Ready(catalog) => (
                        catalog.workspace_uri.clone(),
                        catalog.current.clone(),
                        catalog.branches.contains(branch),
                        catalog.dirty_files,
                    ),
                    _ => return Some(NavigationOutcome::Ignored),
                };
            if &catalog_workspace != workspace_uri
                || !branch_exists
                || state.active_available_workspace() != Some(workspace_uri)
            {
                return Some(NavigationOutcome::Ignored);
            }
            if current == *branch {
                state.composer.selected_branch = Some(branch.clone());
                state.composer.context_menu = None;
                return Some(NavigationOutcome::Applied);
            }
            if dirty_files > 0 {
                return Some(NavigationOutcome::Ignored);
            }
            let workspace_has_active_turn = state.active_turns.keys().any(|session| {
                state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
                    .is_some_and(|thread| &thread.workspace_uri == workspace_uri)
            });
            if workspace_has_active_turn {
                return Some(NavigationOutcome::Ignored);
            }
            state.composer.branch_picker.catalog = BranchCatalogState::Switching {
                workspace_uri: workspace_uri.clone(),
                from: current,
                branch: branch.clone(),
            };
            Some(NavigationOutcome::NeedsEffect)
        }
        AppCommand::CreateProject => {
            close_project_picker(state);
            Some(NavigationOutcome::NeedsEffect)
        }
        _ => None,
    }
}

fn toggle_project_picker(state: &mut ZodeAppState, anchor: ProjectPickerAnchor) {
    if state.project_picker.open && state.project_picker.anchor == anchor {
        close_project_picker(state);
        return;
    }
    state.project_picker = ProjectPickerState {
        open: true,
        anchor,
        search: String::new(),
        active_index: 0,
    };
    state.composer.context_menu = None;
    state.composer.footer_menu = None;
}

fn close_project_picker(state: &mut ZodeAppState) {
    state.project_picker = ProjectPickerState::default();
}

fn reset_composer_task_context(state: &mut ZodeAppState) {
    state.composer.context_menu = None;
    state.composer.footer_menu = None;
    state.composer.launch_mode = TaskLaunchMode::Local;
    state.composer.branch_picker = crate::BranchPickerState::default();
    state.composer.selected_branch = None;
}

fn sandbox_label(mode: zode_node_protocol::SandboxMode, network: bool) -> &'static str {
    match (mode, network) {
        (zode_node_protocol::SandboxMode::ReadOnly, _) => "只读",
        (zode_node_protocol::SandboxMode::WorkspaceWrite, false) => "请求批准",
        (zode_node_protocol::SandboxMode::WorkspaceWrite, true) => "替我审批",
        (zode_node_protocol::SandboxMode::Off, _) => "完全访问",
    }
}

fn approval_label(mode: zode_node_protocol::ApprovalMode) -> &'static str {
    match mode {
        zode_node_protocol::ApprovalMode::Request => "请求批准",
        zode_node_protocol::ApprovalMode::Auto => "替我审批",
        zode_node_protocol::ApprovalMode::Full => "完全访问",
    }
}
