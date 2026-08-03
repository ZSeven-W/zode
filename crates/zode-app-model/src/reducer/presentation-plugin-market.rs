use std::collections::BTreeSet;

use crate::{
    AppCommand, LoadState, PluginAddState, PluginAddStatus, PluginDetailMode, PluginDetailState,
    PluginUpdateState, ShellRoute, ZodeAppState,
};

use super::PresentationCommandOutcome;

/// Applies every plugin-market command that is pure UI state - opening or
/// closing the add-plugin form and its two text fields, opening/closing the
/// detail overlay, and switching the overlay between its Overview /
/// ConfirmUninstall / TrustReview modes. Commands that must reach the
/// endpoint (`InstallPlugin`, `ConfirmUninstallPlugin`, `GrantPluginTrust`)
/// are deliberately left unmatched here so they fall through to
/// `command_bridge::prepare_dispatch` in the desktop app crate.
///
/// `CheckPluginUpdate` is the one hybrid: it is applied here (the overlay
/// goes to `PluginUpdateState::Checking`) and the desktop crate separately
/// turns it into a `PluginUpdateCheck` presentation query, exactly the way
/// `RequestPluginTrustReview` already pairs a local mode switch with an
/// on-demand fetch.
pub(super) fn reduce_plugin_market_presentation_command(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<PresentationCommandOutcome> {
    match command {
        AppCommand::SetPluginAddOpen(open) => {
            if !matches!(state.presentation.route, ShellRoute::Integrations(_)) {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.plugin_add = if *open {
                PluginAddState {
                    open: true,
                    ..Default::default()
                }
            } else {
                PluginAddState::default()
            };
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::SetPluginAddSpec(spec) => {
            if !state.presentation.plugin_add.open {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.plugin_add.spec = spec.clone();
            clear_failed_status(&mut state.presentation.plugin_add.status);
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::SetPluginAddReference(reference) => {
            if !state.presentation.plugin_add.open {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.plugin_add.reference = reference.clone();
            clear_failed_status(&mut state.presentation.plugin_add.status);
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::OpenPluginDetail(plugin_id) => {
            let installed = state
                .presentation
                .installed_plugins
                .ready()
                .is_some_and(|plugins| plugins.iter().any(|plugin| &plugin.id == plugin_id));
            if !installed {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.plugin_detail = Some(PluginDetailState::overview(plugin_id.clone()));
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::ClosePluginDetail => {
            if state.presentation.plugin_detail.is_none() {
                return Some(PresentationCommandOutcome::Ignored);
            }
            state.presentation.plugin_detail = None;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::RequestUninstallPlugin => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            if detail.mode != PluginDetailMode::Overview {
                return Some(PresentationCommandOutcome::Ignored);
            }
            detail.mode = PluginDetailMode::ConfirmUninstall;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::CancelUninstallPlugin => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            if detail.mode != PluginDetailMode::ConfirmUninstall {
                return Some(PresentationCommandOutcome::Ignored);
            }
            detail.mode = PluginDetailMode::Overview;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::CancelPluginTrustReview => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            if !matches!(detail.mode, PluginDetailMode::TrustReview { .. }) {
                return Some(PresentationCommandOutcome::Ignored);
            }
            detail.mode = PluginDetailMode::Overview;
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::RequestPluginTrustReview => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            if detail.mode != PluginDetailMode::Overview {
                return Some(PresentationCommandOutcome::Ignored);
            }
            detail.mode = PluginDetailMode::TrustReview {
                review: LoadState::Loading,
                selected: BTreeSet::new(),
            };
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::ToggleTrustItemSelected(key) => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            let PluginDetailMode::TrustReview { selected, .. } = &mut detail.mode else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            if !selected.remove(key) {
                selected.insert(key.clone());
            }
            Some(PresentationCommandOutcome::Applied)
        }
        AppCommand::CheckPluginUpdate => {
            let Some(detail) = state.presentation.plugin_detail.as_mut() else {
                return Some(PresentationCommandOutcome::Ignored);
            };
            // Only the capability list carries the update buttons, and a
            // second press while git is already running would race the
            // in-flight query's result into a stale state.
            if detail.mode != PluginDetailMode::Overview || detail.update.busy() {
                return Some(PresentationCommandOutcome::Ignored);
            }
            detail.notice = None;
            detail.update = PluginUpdateState::Checking;
            Some(PresentationCommandOutcome::Applied)
        }
        _ => None,
    }
}

fn clear_failed_status(status: &mut PluginAddStatus) {
    if matches!(status, PluginAddStatus::Failed(_)) {
        *status = PluginAddStatus::Idle;
    }
}
