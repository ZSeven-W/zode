use zode_app_model::{AppCommand, PreviewState, PreviewTarget, ZodeAppState};

use crate::services::ExternalOpenService;

pub(super) fn consume_external_preview_command(
    state: &mut ZodeAppState,
    external_open: &dyn ExternalOpenService,
    command: &AppCommand,
) -> bool {
    let AppCommand::OpenPreviewExternally {
        session,
        relative_path,
    } = command
    else {
        return false;
    };
    let valid_session = state.current_session.as_ref() == Some(session)
        && !session.session_id.starts_with("local-error-")
        && state.transcripts.contains_key(session);
    let Some(workspace_uri) = valid_session
        .then(|| state.available_workspace_for_session(session).cloned())
        .flatten()
        .filter(|workspace| workspace.as_str().starts_with("file://"))
    else {
        return true;
    };
    let target = PreviewTarget {
        workspace_uri,
        relative_path: relative_path.clone(),
    };
    let Some(preview) = state
        .presentation
        .sessions
        .get_mut(session)
        .map(|presentation| &mut presentation.preview)
    else {
        return true;
    };
    if preview.target() != Some(&target) {
        return true;
    }
    if let Err(error) = external_open.open_file(&target.workspace_uri, &target.relative_path) {
        *preview = PreviewState::Failed {
            target,
            message: error.to_string(),
        };
    }
    true
}
