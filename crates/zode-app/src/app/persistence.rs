use super::DesktopApp;

impl DesktopApp {
    pub(super) fn persist_ui_state(&self) {
        let Some(store) = self.app_state_store.as_ref() else {
            return;
        };
        let preferences = self.app_state.ui_preferences.clone();
        let geometry = self.window_geometry;
        let last_session = self
            .app_state
            .current_session
            .as_ref()
            .filter(|session| !session.session_id.starts_with("local-error-"))
            .map(|session| session.session_id.clone());
        let task_context = if last_session.is_some() {
            None
        } else if let Some(workspace_uri) = self.app_state.active_available_workspace().cloned() {
            Some(zode_app_runtime::TaskContext::Project { workspace_uri })
        } else {
            Some(zode_app_runtime::TaskContext::Projectless)
        };
        if let Err(error) = store.update(move |state| {
            state.ui_preferences = preferences;
            state.window_geometry = geometry;
            state.last_session = last_session;
            state.task_context = task_context;
        }) {
            eprintln!("zode-app: failed to persist UI state: {error}");
        }
    }
}
