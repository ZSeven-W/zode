use zode_app_model::AppCommand;
use zode_app_runtime::AppStateStore;

use super::DesktopApp;

impl DesktopApp {
    pub(super) fn persist_local_navigation_effect(&self, command: &AppCommand) -> bool {
        let Some(result) =
            persist_navigation_effect(&self.app_state, self.app_state_store.as_ref(), command)
        else {
            return false;
        };
        if let Err(error) = result {
            eprintln!("zode-app: navigation state could not be persisted: {error}");
        }
        true
    }
}

fn persist_navigation_effect(
    app_state: &zode_app_model::ZodeAppState,
    store: Option<&AppStateStore>,
    command: &AppCommand,
) -> Option<Result<(), zode_core::CoreError>> {
    let store = match store {
        Some(store) => store,
        None if is_persisted_navigation_command(command) => return Some(Ok(())),
        None => return None,
    };
    let result = match command {
        AppCommand::ToggleProject(workspace_uri) => {
            let collapsed = app_state
                .projects
                .iter()
                .find(|project| &project.workspace_uri == workspace_uri)
                .is_some_and(|project| !project.expanded);
            let key = workspace_uri.as_str().to_owned();
            store.update(move |state| {
                if collapsed {
                    state.collapsed_workspaces.insert(key);
                } else {
                    state.collapsed_workspaces.remove(&key);
                }
            })
        }
        AppCommand::SetSessionPinned { session, pinned } => {
            let key = session.session_id.clone();
            let pinned = *pinned;
            store.update(move |state| {
                state.sessions.entry(key).or_default().pinned = pinned;
            })
        }
        AppCommand::SetSessionArchived { session, archived } => {
            let key = session.session_id.clone();
            let archived = *archived;
            store.update(move |state| {
                state.sessions.entry(key).or_default().archived = archived;
            })
        }
        AppCommand::ArchiveProjectTasks { workspace_uri } => {
            let session_ids = app_state
                .threads
                .iter()
                .filter(|thread| {
                    app_state.project_workspace_for_thread(thread) == Some(workspace_uri)
                })
                .map(|thread| thread.session.session_id.clone())
                .collect::<Vec<_>>();
            store.update(move |state| {
                for session_id in session_ids {
                    state.sessions.entry(session_id).or_default().archived = true;
                }
            })
        }
        AppCommand::SetProjectPinned {
            workspace_uri,
            pinned,
        } => {
            let workspace_uri = workspace_uri.clone();
            let pinned = *pinned;
            store.update(move |state| {
                if pinned {
                    state.pinned_workspaces.insert(workspace_uri);
                } else {
                    state.pinned_workspaces.remove(&workspace_uri);
                }
            })
        }
        AppCommand::SetProjectDisplayMode(mode) => {
            let mode = *mode;
            store.update(move |state| state.project_display_mode = mode)
        }
        AppCommand::SetProjectSortMode(mode) => {
            let mode = *mode;
            store.update(move |state| state.project_sort_mode = mode)
        }
        _ => return None,
    };
    Some(result)
}

fn is_persisted_navigation_command(command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::ToggleProject(_)
            | AppCommand::SetSessionPinned { .. }
            | AppCommand::SetSessionArchived { .. }
            | AppCommand::ArchiveProjectTasks { .. }
            | AppCommand::SetProjectPinned { .. }
            | AppCommand::SetProjectDisplayMode(_)
            | AppCommand::SetProjectSortMode(_)
    )
}

#[cfg(test)]
mod tests {
    use zode_app_model::{
        AppCommand, ProjectDisplayMode, ProjectSortMode, ProjectState, ZodeAppState,
    };
    use zode_app_runtime::{AppStateFile, AppStateStore, SessionUiState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::persist_navigation_effect;

    #[test]
    fn archive_project_tasks_persists_every_matching_session_only() {
        let (directory, store) = test_store();
        let mut state = zode_app_model::demo_state();
        let target = WorkspaceUri::new("file:///repo/target").unwrap();
        let other = WorkspaceUri::new("file:///repo/other").unwrap();
        add_thread(&mut state, &target, "target-one");
        add_thread(&mut state, &target, "target-two");
        add_thread(&mut state, &other, "other");
        let mut persisted = AppStateFile::default();
        persisted.sessions.insert(
            "other".into(),
            SessionUiState {
                pinned: true,
                ..SessionUiState::default()
            },
        );
        store.save(&persisted).unwrap();

        persist_navigation_effect(
            &state,
            Some(&store),
            &AppCommand::ArchiveProjectTasks {
                workspace_uri: target,
            },
        )
        .unwrap()
        .unwrap();

        let loaded = store.load().unwrap();
        assert!(loaded.sessions["target-one"].archived);
        assert!(loaded.sessions["target-two"].archived);
        assert!(!loaded.sessions["other"].archived);
        assert!(loaded.sessions["other"].pinned);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn project_collapse_pin_display_and_sort_preferences_persist_together() {
        let (directory, store) = test_store();
        let mut state = zode_app_model::demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: false,
            available: true,
            last_opened_ms: 0,
        });

        for command in [
            AppCommand::ToggleProject(workspace.clone()),
            AppCommand::SetProjectPinned {
                workspace_uri: workspace.clone(),
                pinned: true,
            },
            AppCommand::SetProjectDisplayMode(ProjectDisplayMode::Flat),
            AppCommand::SetProjectSortMode(ProjectSortMode::RecentlyUpdated),
        ] {
            persist_navigation_effect(&state, Some(&store), &command)
                .unwrap()
                .unwrap();
        }

        let loaded = store.load().unwrap();
        assert!(loaded.collapsed_workspaces.contains(workspace.as_str()));
        assert!(loaded.pinned_workspaces.contains(&workspace));
        assert_eq!(loaded.project_display_mode, ProjectDisplayMode::Flat);
        assert_eq!(loaded.project_sort_mode, ProjectSortMode::RecentlyUpdated);
        let _ = std::fs::remove_dir_all(directory);
    }

    fn add_thread(state: &mut ZodeAppState, workspace: &WorkspaceUri, session_id: &str) {
        state.threads.push(ThreadSummary {
            session: SessionLocator::new(state.host.node_id, session_id),
            workspace_uri: workspace.clone(),
            title: session_id.into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
    }

    fn test_store() -> (std::path::PathBuf, AppStateStore) {
        let directory = std::env::temp_dir().join(format!(
            "zode-navigation-persistence-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let store = AppStateStore::new(&directory);
        (directory, store)
    }
}
