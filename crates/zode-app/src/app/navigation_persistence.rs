use zode_app_model::AppCommand;

use super::DesktopApp;

impl DesktopApp {
    pub(super) fn persist_local_navigation_effect(&self, command: &AppCommand) -> bool {
        let Some(store) = self.app_state_store.as_ref() else {
            return matches!(
                command,
                AppCommand::ToggleProject(_)
                    | AppCommand::SetSessionPinned { .. }
                    | AppCommand::SetSessionArchived { .. }
            );
        };
        let result = match command {
            AppCommand::ToggleProject(workspace_uri) => {
                let collapsed = self
                    .app_state
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
            _ => return false,
        };
        if let Err(error) = result {
            eprintln!("zode-app: navigation state could not be persisted: {error}");
        }
        true
    }
}
