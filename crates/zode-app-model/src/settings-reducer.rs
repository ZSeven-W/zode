use crate::{AppCommand, ZodeAppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommandOutcome {
    Applied,
    Ignored,
}

pub fn reduce_settings_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> SettingsCommandOutcome {
    match command {
        AppCommand::SetProjectPermissions {
            workspace_uri,
            mut tools,
        } => {
            tools.sort();
            tools.dedup();
            state
                .project_permissions
                .insert(workspace_uri, crate::LoadState::Ready(tools));
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetThemePreference(theme) => {
            state.ui_preferences.theme = theme;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetReducedMotion(reduced_motion) => {
            state.ui_preferences.reduced_motion = reduced_motion;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetHighContrast(high_contrast) => {
            state.ui_preferences.high_contrast = high_contrast;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetTaskSuggestions(visible) => {
            state.ui_preferences.task_suggestions = visible;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSidebarTasksExpanded(expanded) => {
            state.ui_preferences.sidebar_tasks_expanded = expanded;
            state.sidebar.tasks_expanded = expanded;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSettingsSearch(search) => {
            state.settings_search = search;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetArchivedTaskSearch(search) => {
            state.archived_tasks.search = search;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetArchivedTaskWorkspaceFilter(workspace_filter) => {
            if workspace_filter.as_ref().is_some_and(|workspace| {
                !state.threads.iter().any(|thread| {
                    state.project_workspace_for_thread(thread) == Some(workspace)
                        && state.archived_sessions.contains(&thread.session)
                })
            }) {
                return SettingsCommandOutcome::Ignored;
            }
            state.archived_tasks.workspace_filter = workspace_filter;
            state.settings_scroll_offset = 0.0;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSettingsScroll { offset } if offset.is_finite() => {
            state.settings_scroll_offset = offset.max(0.0);
            SettingsCommandOutcome::Applied
        }
        AppCommand::LoadProviderModels => {
            state.provider_models.load = crate::LoadState::Loading;
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::ProviderModelsLoaded(summaries) => {
            state.provider_models.load =
                crate::LoadState::Ready(summaries.into_iter().map(normalize_summary).collect());
            SettingsCommandOutcome::Applied
        }
        AppCommand::ProviderModelsLoadFailed(message) => {
            state.provider_models.load = crate::LoadState::Failed(message);
            SettingsCommandOutcome::Applied
        }
        AppCommand::BeginProviderEdit {
            draft,
            credential_configured,
        } => {
            if state.provider_models.status.is_busy() {
                return SettingsCommandOutcome::Ignored;
            }
            state.provider_models.editor = Some(draft);
            state.provider_models.credential_configured = credential_configured;
            state.provider_models.pending_secret_len = 0;
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::CancelProviderEdit => {
            if state.provider_models.editor.is_none() || state.provider_models.status.is_busy() {
                return SettingsCommandOutcome::Ignored;
            }
            state.provider_models.editor = None;
            state.provider_models.credential_configured = false;
            state.provider_models.pending_secret_len = 0;
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::UpdateProviderDraft(draft) => {
            if state.provider_models.editor.is_none() || state.provider_models.status.is_busy() {
                return SettingsCommandOutcome::Ignored;
            }
            state.provider_models.editor = Some(draft);
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetProviderDraftKind(kind) => {
            let Some(draft) = editable_provider_draft(state) else {
                return SettingsCommandOutcome::Ignored;
            };
            draft.kind = kind;
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetProviderDraftBaseUrl(base_url) => {
            let Some(draft) = editable_provider_draft(state) else {
                return SettingsCommandOutcome::Ignored;
            };
            draft.base_url = base_url;
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetProviderDraftModelIds(model_ids) => {
            let Some(draft) = editable_provider_draft(state) else {
                return SettingsCommandOutcome::Ignored;
            };
            draft.model_ids = model_ids;
            if draft
                .default_model
                .as_ref()
                .is_some_and(|selected| !draft.model_ids.iter().any(|model| model == selected))
            {
                draft.default_model = None;
            }
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SelectProviderDefaultModel(model_id) => {
            let Some(draft) = editable_provider_draft(state) else {
                return SettingsCommandOutcome::Ignored;
            };
            if !draft.model_ids.iter().any(|model| model == &model_id) {
                return SettingsCommandOutcome::Ignored;
            }
            draft.default_model = Some(model_id);
            state.provider_models.status = crate::ProviderModelsStatus::Idle;
            SettingsCommandOutcome::Applied
        }
        AppCommand::RequestSaveProvider => request_provider_save(state),
        AppCommand::ProviderSaveSucceeded(summary) => provider_save_succeeded(state, summary),
        AppCommand::ProviderSaveFailed {
            provider_id,
            message,
        } => provider_save_failed(state, provider_id, message),
        AppCommand::RequestRemoveProvider { provider_id } => {
            let provider_id = provider_id.trim();
            let provider_exists = matches!(
                &state.provider_models.load,
                crate::LoadState::Ready(summaries)
                    if summaries.iter().any(|summary| summary.provider_id == provider_id)
            );
            if provider_id.is_empty() || !provider_exists || state.provider_models.status.is_busy()
            {
                return SettingsCommandOutcome::Ignored;
            }
            state.provider_models.status = crate::ProviderModelsStatus::Removing {
                provider_id: provider_id.to_owned(),
            };
            SettingsCommandOutcome::Applied
        }
        AppCommand::ProviderRemoveSucceeded { provider_id } => {
            provider_remove_succeeded(state, provider_id)
        }
        AppCommand::ProviderRemoveFailed {
            provider_id,
            message,
        } => provider_remove_failed(state, provider_id, message),
        AppCommand::RevokeProjectPermission {
            workspace_uri,
            tool,
        } => {
            let Some(crate::LoadState::Ready(tools)) =
                state.project_permissions.get_mut(&workspace_uri)
            else {
                return SettingsCommandOutcome::Ignored;
            };
            let previous_len = tools.len();
            tools.retain(|candidate| candidate != &tool);
            if tools.len() == previous_len {
                return SettingsCommandOutcome::Ignored;
            }
            SettingsCommandOutcome::Applied
        }
        _ => SettingsCommandOutcome::Ignored,
    }
}

fn editable_provider_draft(state: &mut ZodeAppState) -> Option<&mut crate::ProviderDraft> {
    if state.provider_models.status.is_busy() {
        return None;
    }
    state.provider_models.editor.as_mut()
}

fn request_provider_save(state: &mut ZodeAppState) -> SettingsCommandOutcome {
    if state.provider_models.status.is_busy() {
        return SettingsCommandOutcome::Ignored;
    }
    let Some(mut draft) = state.provider_models.editor.take() else {
        return SettingsCommandOutcome::Ignored;
    };
    normalize_draft(&mut draft);
    let validation_error = validate_draft(&draft);
    let provider_id = (!draft.provider_id.is_empty()).then(|| draft.provider_id.clone());
    state.provider_models.editor = Some(draft);
    if let Some(message) = validation_error {
        state.provider_models.status = crate::ProviderModelsStatus::Failed {
            provider_id,
            message: message.to_owned(),
        };
        return SettingsCommandOutcome::Applied;
    }
    state.provider_models.status = crate::ProviderModelsStatus::Saving {
        provider_id: provider_id.expect("a valid provider draft has an id"),
    };
    SettingsCommandOutcome::Applied
}

fn provider_save_succeeded(
    state: &mut ZodeAppState,
    summary: crate::ProviderSummary,
) -> SettingsCommandOutcome {
    let summary = normalize_summary(summary);
    let crate::ProviderModelsStatus::Saving { provider_id } = &state.provider_models.status else {
        return SettingsCommandOutcome::Ignored;
    };
    if provider_id != &summary.provider_id {
        return SettingsCommandOutcome::Ignored;
    }
    let saved_id = summary.provider_id.clone();
    let credential_configured = summary.credential_configured;
    let saved_draft = crate::ProviderDraft::from(&summary);
    match &mut state.provider_models.load {
        crate::LoadState::Ready(summaries) => {
            if let Some(existing) = summaries
                .iter_mut()
                .find(|candidate| candidate.provider_id == saved_id)
            {
                *existing = summary;
            } else {
                summaries.push(summary);
            }
        }
        _ => state.provider_models.load = crate::LoadState::Ready(vec![summary]),
    }
    state.provider_models.editor = Some(saved_draft);
    state.provider_models.credential_configured = credential_configured;
    state.provider_models.pending_secret_len = 0;
    state.provider_models.status = crate::ProviderModelsStatus::Saved {
        provider_id: saved_id,
    };
    SettingsCommandOutcome::Applied
}

fn provider_save_failed(
    state: &mut ZodeAppState,
    provider_id: String,
    message: String,
) -> SettingsCommandOutcome {
    let crate::ProviderModelsStatus::Saving {
        provider_id: pending,
    } = &state.provider_models.status
    else {
        return SettingsCommandOutcome::Ignored;
    };
    if pending != &provider_id {
        return SettingsCommandOutcome::Ignored;
    }
    state.provider_models.status = crate::ProviderModelsStatus::Failed {
        provider_id: Some(provider_id),
        message,
    };
    SettingsCommandOutcome::Applied
}

fn provider_remove_succeeded(
    state: &mut ZodeAppState,
    provider_id: String,
) -> SettingsCommandOutcome {
    let crate::ProviderModelsStatus::Removing {
        provider_id: pending,
    } = &state.provider_models.status
    else {
        return SettingsCommandOutcome::Ignored;
    };
    if pending != &provider_id {
        return SettingsCommandOutcome::Ignored;
    }
    if let crate::LoadState::Ready(summaries) = &mut state.provider_models.load {
        summaries.retain(|summary| summary.provider_id != provider_id);
    }
    if state
        .provider_models
        .editor
        .as_ref()
        .is_some_and(|draft| draft.provider_id == provider_id)
    {
        state.provider_models.editor = None;
        state.provider_models.credential_configured = false;
        state.provider_models.pending_secret_len = 0;
    }
    state.provider_models.status = crate::ProviderModelsStatus::Removed { provider_id };
    SettingsCommandOutcome::Applied
}

fn provider_remove_failed(
    state: &mut ZodeAppState,
    provider_id: String,
    message: String,
) -> SettingsCommandOutcome {
    let crate::ProviderModelsStatus::Removing {
        provider_id: pending,
    } = &state.provider_models.status
    else {
        return SettingsCommandOutcome::Ignored;
    };
    if pending != &provider_id {
        return SettingsCommandOutcome::Ignored;
    }
    state.provider_models.status = crate::ProviderModelsStatus::Failed {
        provider_id: Some(provider_id),
        message,
    };
    SettingsCommandOutcome::Applied
}

fn normalize_summary(mut summary: crate::ProviderSummary) -> crate::ProviderSummary {
    summary.provider_id = summary.provider_id.trim().to_owned();
    summary.base_url = normalize_optional_text(summary.base_url);
    let mut model_ids: Vec<String> = summary
        .models
        .into_iter()
        .map(|entry| entry.model_id)
        .collect();
    normalize_model_ids(&mut model_ids);
    summary.models = model_ids
        .into_iter()
        .map(|model_id| crate::ProviderModelEntry { model_id })
        .collect();
    summary.default_model = normalize_optional_text(summary.default_model).filter(|selected| {
        summary
            .models
            .iter()
            .any(|model| &model.model_id == selected)
    });
    summary
}

fn normalize_draft(draft: &mut crate::ProviderDraft) {
    draft.provider_id = draft.provider_id.trim().to_owned();
    draft.base_url = normalize_optional_text(draft.base_url.take());
    normalize_model_ids(&mut draft.model_ids);
    draft.default_model = normalize_optional_text(draft.default_model.take());
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_model_ids(model_ids: &mut Vec<String>) {
    let mut normalized = Vec::with_capacity(model_ids.len());
    for model_id in model_ids.drain(..) {
        let model_id = model_id.trim();
        if !model_id.is_empty() && !normalized.iter().any(|candidate| candidate == model_id) {
            normalized.push(model_id.to_owned());
        }
    }
    *model_ids = normalized;
}

fn validate_draft(draft: &crate::ProviderDraft) -> Option<&'static str> {
    if draft.provider_id.is_empty() {
        return Some("provider id is required");
    }
    if draft.model_ids.is_empty() {
        return Some("at least one model is required");
    }
    let Some(default_model) = draft.default_model.as_ref() else {
        return Some("a default model is required");
    };
    if !draft.model_ids.iter().any(|model| model == default_model) {
        return Some("the default model must belong to the provider");
    }
    None
}
