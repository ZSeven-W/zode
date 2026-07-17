use crate::{AppCommand, NavigationOutcome, SessionRenameState, ZodeAppState};

/// Applies task-title menu and rename-dialog state without coupling those
/// transient surfaces to the endpoint command bridge.
pub(crate) fn reduce_session_navigation(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<NavigationOutcome> {
    let session = match command {
        AppCommand::ToggleSessionMenu { session }
        | AppCommand::ToggleSessionCopyMenu { session }
        | AppCommand::BeginRenameSession { session }
        | AppCommand::SetSessionRenameDraft { session, .. }
        | AppCommand::CancelRenameSession { session }
        | AppCommand::OpenSessionInNewWindow { session }
        | AppCommand::RenameSession { session, .. } => session,
        _ => return None,
    };
    if state.current_session.as_ref() != Some(session)
        || !state
            .threads
            .iter()
            .any(|thread| &thread.session == session)
    {
        return Some(NavigationOutcome::Ignored);
    }

    let outcome = match command {
        AppCommand::ToggleSessionMenu { session } => {
            let opening = state.session_menu.as_ref() != Some(session);
            state.close_session_action_surfaces();
            if opening {
                state.session_menu = Some(session.clone());
                state.composer.queue_menu = None;
            }
            NavigationOutcome::Applied
        }
        AppCommand::ToggleSessionCopyMenu { session } => {
            if state.session_menu.as_ref() != Some(session) {
                return Some(NavigationOutcome::Ignored);
            }
            state.session_copy_menu =
                (state.session_copy_menu.as_ref() != Some(session)).then(|| session.clone());
            NavigationOutcome::Applied
        }
        AppCommand::BeginRenameSession { session } => {
            let Some(title) = state
                .threads
                .iter()
                .find(|thread| &thread.session == session)
                .map(|thread| thread.title.clone())
            else {
                return Some(NavigationOutcome::Ignored);
            };
            state.close_session_action_surfaces();
            state.session_rename = Some(SessionRenameState {
                session: session.clone(),
                draft: title,
            });
            NavigationOutcome::Applied
        }
        AppCommand::SetSessionRenameDraft { session, draft } => {
            let Some(rename) = state
                .session_rename
                .as_mut()
                .filter(|rename| &rename.session == session)
            else {
                return Some(NavigationOutcome::Ignored);
            };
            rename.draft.clone_from(draft);
            NavigationOutcome::Applied
        }
        AppCommand::CancelRenameSession { session } => {
            if state.session_rename.as_ref().map(|rename| &rename.session) != Some(session) {
                return Some(NavigationOutcome::Ignored);
            }
            state.session_rename = None;
            NavigationOutcome::Applied
        }
        AppCommand::OpenSessionInNewWindow { .. } => NavigationOutcome::NeedsEffect,
        AppCommand::RenameSession { session, title } => {
            let title = title.trim();
            if title.is_empty() {
                return Some(NavigationOutcome::Ignored);
            }
            let Some(thread) = state
                .threads
                .iter_mut()
                .find(|thread| &thread.session == session)
            else {
                return Some(NavigationOutcome::Ignored);
            };
            thread.title = title.to_owned();
            state.close_session_action_surfaces();
            NavigationOutcome::NeedsEffect
        }
        _ => unreachable!("the command was matched above"),
    };
    Some(outcome)
}
