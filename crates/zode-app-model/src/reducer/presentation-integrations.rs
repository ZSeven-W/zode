use crate::{AppCommand, ShellRoute, ZodeAppState};

use super::PresentationCommandOutcome;

pub(super) fn reduce_integration_presentation_command(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<PresentationCommandOutcome> {
    match command {
        AppCommand::SetIntegrationSearch(search) => {
            if !matches!(state.presentation.route, ShellRoute::Integrations(_)) {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.integration_search = search.clone();
            state.integration_scroll_offset = 0.0;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::SetIntegrationScope(scope) => {
            if !matches!(state.presentation.route, ShellRoute::Integrations(_)) {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.integration_scope = *scope;
            state.integration_scroll_offset = 0.0;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::SetIntegrationsScroll { offset } => {
            if !matches!(state.presentation.route, ShellRoute::Integrations(_))
                || !offset.is_finite()
            {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.integration_scroll_offset = offset.max(0.0);
            Some(PresentationCommandOutcome::Applied)
        }
        _ => None,
    }
}
