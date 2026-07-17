use zode_app_model::{reduce_presentation_command, AppCommand, ShellRoute, ZodeAppState};

pub(in crate::app) fn normalize_conversation_route(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> bool {
    if !matches!(
        command,
        AppCommand::SelectSession(_) | AppCommand::NewSession { .. } | AppCommand::BeginTask { .. }
    ) {
        return false;
    }
    let _ = reduce_presentation_command(state, AppCommand::CloseSecondary);
    let _ = reduce_presentation_command(state, AppCommand::SetPinnedSummaryOverlayOpen(false));
    let _ = reduce_presentation_command(state, AppCommand::Navigate(ShellRoute::Conversation));
    true
}

pub(super) fn available_new_session_command(state: &ZodeAppState, command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::NewSession { workspace_uri } if state.available_workspace(workspace_uri)
    )
}
