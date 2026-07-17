use zode_app_model::ZodeAppState;
use zode_app_runtime::AppStateFile;

pub(super) fn local_display_name_from_env() -> String {
    display_name_from_candidates([std::env::var("USER").ok(), std::env::var("USERNAME").ok()])
}

fn display_name_from_candidates<I>(candidates: I) -> String
where
    I: IntoIterator<Item = Option<String>>,
{
    candidates
        .into_iter()
        .flatten()
        .find_map(|candidate| normalize_display_name(&candidate))
        .unwrap_or_else(|| "本地".into())
}

fn normalize_display_name(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.rsplit(['\\', '/']).next().unwrap_or(value);
    let value = value.split('@').next().unwrap_or(value).trim();
    let mut chars = value.chars();
    let first = chars.next()?;
    let mut display_name = first.to_uppercase().collect::<String>();
    display_name.extend(chars);
    Some(display_name)
}

pub(super) fn hydrate_session_navigation(state: &mut ZodeAppState, persisted: &AppStateFile) {
    state.hydrate_thread_affiliations(
        &persisted.thread_workspace_root_hints,
        &persisted.projectless_session_ids,
    );
    state.pinned_sessions.clear();
    state.archived_sessions.clear();
    for thread in &state.threads {
        let Some(ui_state) = persisted.sessions.get(&thread.session.session_id) else {
            continue;
        };
        if ui_state.pinned {
            state.pinned_sessions.insert(thread.session.clone());
        }
        if ui_state.archived {
            state.archived_sessions.insert(thread.session.clone());
        }
    }
    if state
        .current_session
        .as_ref()
        .is_some_and(|session| state.archived_sessions.contains(session))
    {
        state.current_session = None;
    }
}

pub(super) fn hydrate_project_navigation(state: &mut ZodeAppState, persisted: &AppStateFile) {
    for project in &mut state.projects {
        project.expanded = !persisted
            .collapsed_workspaces
            .contains(project.workspace_uri.as_str());
    }
    state.sidebar.pinned_projects = persisted.pinned_workspaces.clone();
    state.sidebar.project_display_mode = persisted.project_display_mode;
    state.sidebar.project_sort_mode = persisted.project_sort_mode;
}

pub(super) fn restore_last_session(state: &mut ZodeAppState, last_session: Option<&str>) {
    let Some(last_session) = last_session else {
        return;
    };
    let restored = state
        .threads
        .iter()
        .find(|thread| {
            thread.session.session_id == last_session
                && !state.archived_sessions.contains(&thread.session)
                && state
                    .available_workspace_for_session(&thread.session)
                    .is_some()
        })
        .map(|thread| thread.session.clone());
    let Some(session) = restored else {
        return;
    };
    let project_workspace = state.project_workspace_for_session(&session).cloned();
    state.current_session = Some(session);
    state.active_workspace =
        project_workspace.filter(|workspace| state.available_workspace(workspace));
}

#[cfg(test)]
mod tests {
    use zode_app_model::{demo_state, ProjectDisplayMode, ProjectSortMode, ProjectState};
    use zode_app_runtime::{AppStateFile, SessionUiState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::{
        display_name_from_candidates, hydrate_project_navigation, hydrate_session_navigation,
        restore_last_session,
    };

    #[test]
    fn local_profile_uses_first_available_username_and_capitalizes_it() {
        assert_eq!(
            display_name_from_candidates([Some("  ".into()), Some("domain\\fini".into())]),
            "Fini"
        );
        assert_eq!(display_name_from_candidates([None, None]), "本地");
    }

    #[test]
    fn persisted_navigation_metadata_is_hydrated_only_for_real_threads() {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "real");
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace,
            title: "real".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state.current_session = Some(session.clone());
        let mut persisted = AppStateFile::default();
        for id in ["real", "stale"] {
            persisted.sessions.insert(
                id.into(),
                SessionUiState {
                    pinned: true,
                    archived: true,
                    unread: false,
                    failed: false,
                },
            );
        }

        hydrate_session_navigation(&mut state, &persisted);

        assert_eq!(state.pinned_sessions, [session.clone()].into());
        assert_eq!(state.archived_sessions, [session].into());
        assert_eq!(state.current_session, None);

        restore_last_session(&mut state, Some("real"));
        assert_eq!(state.current_session, None);
    }

    #[test]
    fn persisted_project_preferences_are_hydrated_on_launch() {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        let mut persisted = AppStateFile::default();
        persisted
            .collapsed_workspaces
            .insert(workspace.as_str().into());
        persisted.pinned_workspaces.insert(workspace.clone());
        persisted.project_display_mode = ProjectDisplayMode::Flat;
        persisted.project_sort_mode = ProjectSortMode::Manual;

        hydrate_project_navigation(&mut state, &persisted);

        assert!(!state.projects[0].expanded);
        assert_eq!(state.sidebar.pinned_projects, [workspace].into());
        assert_eq!(state.sidebar.project_display_mode, ProjectDisplayMode::Flat);
        assert_eq!(state.sidebar.project_sort_mode, ProjectSortMode::Manual);
    }
}
