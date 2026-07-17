use std::{mem, path::PathBuf};

use zode_app_model::{
    reduce_settings_command, AppCommand, LoadState, ProviderDraft, ProviderKindChoice,
    ProviderModelEntry, ProviderModelsStatus, ProviderSummary, SettingsCommandOutcome,
    ZodeAppState,
};
use zode_app_ui::{ImeEvent, KeyEvent, WidgetId, PROVIDER_API_KEY_INPUT_ID};
use zode_core::config::{ConfigManager, ModelOverride, ProviderConfig, ProviderKind, ZodeConfig};

use super::provider_input::{ProviderInputController, ProviderInputOutcome};

/// Result of handling a provider-settings command at the persistence boundary.
pub(super) enum ProviderModelsCommandOutcome {
    NotHandled(AppCommand),
    Handled,
    Forward(AppCommand),
}

#[derive(Clone, Copy, Default)]
struct CredentialEnvironment {
    anthropic: bool,
    openai: bool,
}

impl CredentialEnvironment {
    fn from_process() -> Self {
        Self {
            anthropic: environment_value_is_configured("ANTHROPIC_API_KEY"),
            openai: environment_value_is_configured("OPENAI_API_KEY"),
        }
    }

    const fn configured_for(self, kind: ProviderKindChoice) -> bool {
        match kind {
            ProviderKindChoice::Anthropic => self.anthropic,
            ProviderKindChoice::OpenAi => self.openai,
            ProviderKindChoice::Ollama => true,
        }
    }
}

fn environment_value_is_configured(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

/// Owns the only in-memory API-key draft. Secrets never enter `ZodeAppState`;
/// snapshots and accessibility expose only `credential_configured`.
pub(super) struct ProviderModelsController {
    config_dir: Result<PathBuf, String>,
    inputs: ProviderInputController,
    original_provider_id: Option<String>,
    original_provider_kind: Option<ProviderKindChoice>,
    stored_credential_configured: bool,
    environment: CredentialEnvironment,
}

impl Default for ProviderModelsController {
    fn default() -> Self {
        Self {
            config_dir: ConfigManager::config_dir().map_err(|error| error.to_string()),
            inputs: ProviderInputController::default(),
            original_provider_id: None,
            original_provider_kind: None,
            stored_credential_configured: false,
            environment: CredentialEnvironment::from_process(),
        }
    }
}

impl ProviderModelsController {
    #[cfg(test)]
    fn with_config_dir(config_dir: PathBuf) -> Self {
        Self {
            config_dir: Ok(config_dir),
            inputs: ProviderInputController::default(),
            original_provider_id: None,
            original_provider_kind: None,
            stored_credential_configured: false,
            environment: CredentialEnvironment::default(),
        }
    }

    fn config_dir(&self) -> Result<&std::path::Path, String> {
        self.config_dir
            .as_deref()
            .map_err(|message| message.clone())
    }

    pub(super) fn secret_input_value(&self) -> &str {
        self.inputs
            .text(PROVIDER_API_KEY_INPUT_ID)
            .unwrap_or_default()
    }

    /// Character count of the API-key input buffer. A non-sensitive length
    /// projection only; the secret text itself never leaves the controller.
    pub(super) fn pending_secret_len(&self) -> usize {
        self.secret_input_value().chars().count()
    }

    #[cfg(test)]
    pub(super) fn set_secret_input_value(
        &mut self,
        value: String,
        kind: ProviderKindChoice,
    ) -> bool {
        let _ = self.inputs.set_text(PROVIDER_API_KEY_INPUT_ID, value);
        self.credential_configured_for(kind)
    }

    pub(super) fn input_value(&self, id: WidgetId) -> Option<&str> {
        self.inputs.text(id)
    }

    pub(super) fn set_input_value(&mut self, id: WidgetId, value: String) -> bool {
        self.inputs.set_text(id, value)
    }

    pub(super) fn edit_input_key(
        &mut self,
        id: WidgetId,
        event: &KeyEvent,
    ) -> ProviderInputOutcome {
        self.inputs.key(id, event)
    }

    pub(super) fn edit_input_ime(
        &mut self,
        id: WidgetId,
        event: &ImeEvent,
    ) -> ProviderInputOutcome {
        self.inputs.ime(id, event)
    }

    pub(super) fn paste_input(&mut self, id: WidgetId, text: &str) -> ProviderInputOutcome {
        self.inputs.paste(id, text)
    }

    pub(super) fn refresh(&mut self, state: &mut ZodeAppState) {
        let _ = reduce_settings_command(state, AppCommand::LoadProviderModels);
        match self.load_summaries() {
            Ok(summaries) => {
                state.provider_setup_required = !summaries.iter().any(provider_is_usable);
                let _ = reduce_settings_command(state, AppCommand::ProviderModelsLoaded(summaries));
            }
            Err(message) => {
                let _ =
                    reduce_settings_command(state, AppCommand::ProviderModelsLoadFailed(message));
            }
        }
    }

    pub(super) fn consume(
        &mut self,
        state: &mut ZodeAppState,
        command: AppCommand,
    ) -> ProviderModelsCommandOutcome {
        match command {
            AppCommand::LoadProviderModels => {
                self.refresh(state);
                ProviderModelsCommandOutcome::Handled
            }
            AppCommand::BeginProviderEdit {
                draft,
                credential_configured,
            } => {
                let input_draft = draft.clone();
                let original = self
                    .current_summaries(state)
                    .into_iter()
                    .find(|summary| summary.provider_id == draft.provider_id);
                let applied = reduce_settings_command(
                    state,
                    AppCommand::BeginProviderEdit {
                        draft,
                        credential_configured,
                    },
                );
                if applied == SettingsCommandOutcome::Applied {
                    self.inputs.begin(&input_draft);
                    self.stored_credential_configured = credential_configured;
                    self.original_provider_id =
                        original.as_ref().map(|summary| summary.provider_id.clone());
                    self.original_provider_kind = original.map(|summary| summary.kind);
                }
                ProviderModelsCommandOutcome::Handled
            }
            AppCommand::CancelProviderEdit => {
                if reduce_settings_command(state, AppCommand::CancelProviderEdit)
                    == SettingsCommandOutcome::Applied
                {
                    self.clear_secret();
                }
                ProviderModelsCommandOutcome::Handled
            }
            AppCommand::RequestSaveProvider => self.request_save(state),
            AppCommand::RequestRemoveProvider { provider_id } => {
                self.request_remove(state, provider_id)
            }
            AppCommand::SetProviderDraftKind(kind) => {
                let applied =
                    reduce_settings_command(state, AppCommand::SetProviderDraftKind(kind));
                if applied == SettingsCommandOutcome::Applied {
                    state.provider_models.credential_configured =
                        self.credential_configured_for(kind);
                }
                ProviderModelsCommandOutcome::Handled
            }
            command @ (AppCommand::ProviderModelsLoaded(_)
            | AppCommand::ProviderModelsLoadFailed(_)
            | AppCommand::UpdateProviderDraft(_)
            | AppCommand::SetProviderDraftBaseUrl(_)
            | AppCommand::SetProviderDraftModelIds(_)
            | AppCommand::SelectProviderDefaultModel(_)
            | AppCommand::ProviderSaveSucceeded(_)
            | AppCommand::ProviderSaveFailed { .. }
            | AppCommand::ProviderRemoveSucceeded { .. }
            | AppCommand::ProviderRemoveFailed { .. }) => {
                reduce_provider_state_command(state, command)
            }
            command => ProviderModelsCommandOutcome::NotHandled(command),
        }
    }

    fn request_save(&mut self, state: &mut ZodeAppState) -> ProviderModelsCommandOutcome {
        if reduce_settings_command(state, AppCommand::RequestSaveProvider)
            != SettingsCommandOutcome::Applied
        {
            return ProviderModelsCommandOutcome::Handled;
        }
        if !matches!(
            state.provider_models.status,
            ProviderModelsStatus::Saving { .. }
        ) {
            return ProviderModelsCommandOutcome::Handled;
        }
        let Some(draft) = state.provider_models.editor.clone() else {
            return ProviderModelsCommandOutcome::Handled;
        };
        let provider_id = draft.provider_id.clone();
        let saved = self.persist_provider(&draft);
        match saved {
            Ok(summary) => {
                if let Some(previous_id) = self
                    .original_provider_id
                    .as_deref()
                    .filter(|previous_id| *previous_id != summary.provider_id)
                {
                    if let LoadState::Ready(summaries) = &mut state.provider_models.load {
                        summaries.retain(|entry| entry.provider_id != previous_id);
                    }
                }
                self.inputs.begin(&ProviderDraft::from(&summary));
                self.original_provider_id = Some(summary.provider_id.clone());
                self.original_provider_kind = Some(summary.kind);
                self.stored_credential_configured = summary.credential_configured;
                let _ = reduce_settings_command(state, AppCommand::ProviderSaveSucceeded(summary));
                state.provider_setup_required =
                    !self.current_summaries(state).iter().any(provider_is_usable);
                ProviderModelsCommandOutcome::Forward(AppCommand::ReloadProviderConfiguration)
            }
            Err(message) => {
                let _ = reduce_settings_command(
                    state,
                    AppCommand::ProviderSaveFailed {
                        provider_id,
                        message,
                    },
                );
                ProviderModelsCommandOutcome::Handled
            }
        }
    }

    fn request_remove(
        &mut self,
        state: &mut ZodeAppState,
        provider_id: String,
    ) -> ProviderModelsCommandOutcome {
        if reduce_settings_command(
            state,
            AppCommand::RequestRemoveProvider {
                provider_id: provider_id.clone(),
            },
        ) != SettingsCommandOutcome::Applied
        {
            return ProviderModelsCommandOutcome::Handled;
        }
        match self.remove_provider(&provider_id) {
            Ok(()) => {
                if self.original_provider_id.as_deref() == Some(provider_id.as_str()) {
                    self.clear_secret();
                }
                let _ = reduce_settings_command(
                    state,
                    AppCommand::ProviderRemoveSucceeded { provider_id },
                );
                state.provider_setup_required =
                    !self.current_summaries(state).iter().any(provider_is_usable);
                ProviderModelsCommandOutcome::Forward(AppCommand::ReloadProviderConfiguration)
            }
            Err(message) => {
                let _ = reduce_settings_command(
                    state,
                    AppCommand::ProviderRemoveFailed {
                        provider_id,
                        message,
                    },
                );
                ProviderModelsCommandOutcome::Handled
            }
        }
    }

    fn persist_provider(&self, draft: &ProviderDraft) -> Result<ProviderSummary, String> {
        let config_dir = self.config_dir()?;
        let mut config = ConfigManager::load_global_in(config_dir)
            .map_err(|error| format!("读取 Provider 配置失败：{error}"))?;
        config.normalize_legacy();
        let original_id = self.original_provider_id.as_deref();
        if original_id != Some(draft.provider_id.as_str())
            && config.providers.contains_key(&draft.provider_id)
        {
            return Err("Provider ID 已存在，请换一个名称".into());
        }
        ensure_models_are_unambiguous(&config, original_id, draft)?;

        let synthetic_id = synthetic_provider_id(&config.provider);
        let legacy_active = config.providers.is_empty()
            && original_id == Some(synthetic_id.as_str())
            && config.provider_has_user_data();
        let existing = original_id
            .and_then(|provider_id| config.providers.get(provider_id).cloned())
            .or_else(|| legacy_active.then(|| config.provider.clone()))
            .unwrap_or_default();
        let kind = provider_kind(draft.kind);
        let kind_changed = existing.kind() != kind;
        let api_key = (kind != ProviderKind::Ollama)
            .then(|| {
                nonempty_secret(self.secret_input_value())
                    .or_else(|| (!kind_changed).then(|| existing.api_key.clone()).flatten())
            })
            .flatten();
        if kind != ProviderKind::Ollama
            && api_key.is_none()
            && !self.environment.configured_for(draft.kind)
        {
            return Err(match kind {
                ProviderKind::Anthropic => {
                    "请填写 API Key，或先配置 ANTHROPIC_API_KEY 环境变量".into()
                }
                ProviderKind::Openai => "请填写 API Key，或先配置 OPENAI_API_KEY 环境变量".into(),
                ProviderKind::Ollama => unreachable!(),
            });
        }

        let entry = updated_provider_entry(existing, draft, kind, api_key);
        replace_provider_entry(&mut config, original_id, draft.provider_id.clone(), entry);
        config.set_active_model(
            draft
                .default_model
                .as_deref()
                .expect("the reducer validates a default model before persistence"),
        );
        ConfigManager::save_global_in(config_dir, &config)
            .map_err(|error| format!("保存 Provider 配置失败：{error}"))?;
        self.summaries_from_config(&config)
            .into_iter()
            .find(|summary| summary.provider_id == draft.provider_id)
            .ok_or_else(|| "Provider 已保存，但无法重新读取".into())
    }

    fn remove_provider(&self, provider_id: &str) -> Result<(), String> {
        let config_dir = self.config_dir()?;
        let mut config = ConfigManager::load_global_in(config_dir)
            .map_err(|error| format!("读取 Provider 配置失败：{error}"))?;
        config.normalize_legacy();
        let active_provider = config.active_provider_key().map(ToOwned::to_owned);
        let removed = config.providers.shift_remove(provider_id).is_some();
        let removed_legacy = !removed
            && config.providers.is_empty()
            && config.provider_has_user_data()
            && synthetic_provider_id(&config.provider) == provider_id;
        if !removed && !removed_legacy {
            return Err("找不到要移除的 Provider".into());
        }

        if removed_legacy {
            config.provider = ProviderConfig::default();
        } else if active_provider.as_deref() == Some(provider_id) {
            config.provider = ProviderConfig::default();
            if let Some(next_model) = config.providers.values().find_map(first_model_id) {
                config.set_active_model(&next_model);
            }
        }
        if config.providers.is_empty() && removed {
            config.provider = ProviderConfig::default();
        }
        ConfigManager::save_global_in(config_dir, &config)
            .map_err(|error| format!("保存 Provider 配置失败：{error}"))
    }

    fn load_summaries(&self) -> Result<Vec<ProviderSummary>, String> {
        let mut config = ConfigManager::load_global_in(self.config_dir()?)
            .map_err(|error| format!("读取 Provider 配置失败：{error}"))?;
        config.normalize_legacy();
        Ok(self.summaries_from_config(&config))
    }

    fn summaries_from_config(&self, config: &ZodeConfig) -> Vec<ProviderSummary> {
        if config.providers.is_empty() {
            if !config.provider_has_user_data() {
                return Vec::new();
            }
            return vec![self.summary_for_entry(
                synthetic_provider_id(&config.provider),
                &config.provider,
                config.provider.model.clone(),
            )];
        }
        let active_provider = config.active_provider_key();
        let active_model = config.provider.model.as_deref();
        config
            .providers
            .iter()
            .map(|(provider_id, entry)| {
                let default_model = if active_provider == Some(provider_id.as_str()) {
                    active_model.map(ToOwned::to_owned)
                } else {
                    first_model_id(entry)
                };
                self.summary_for_entry(provider_id.clone(), entry, default_model)
            })
            .collect()
    }

    fn summary_for_entry(
        &self,
        provider_id: String,
        entry: &ProviderConfig,
        default_model: Option<String>,
    ) -> ProviderSummary {
        let kind = provider_kind_choice(entry.kind());
        ProviderSummary {
            provider_id,
            kind,
            base_url: entry.base_url.clone(),
            models: provider_model_ids(entry)
                .into_iter()
                .map(|model_id| ProviderModelEntry { model_id })
                .collect(),
            default_model,
            credential_configured: entry
                .api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
                || self.environment.configured_for(kind),
        }
    }

    fn current_summaries(&self, state: &ZodeAppState) -> Vec<ProviderSummary> {
        match &state.provider_models.load {
            LoadState::Ready(summaries) => summaries.clone(),
            _ => Vec::new(),
        }
    }

    fn clear_secret(&mut self) {
        self.inputs.clear();
        self.original_provider_id = None;
        self.original_provider_kind = None;
        self.stored_credential_configured = false;
    }

    pub(super) fn credential_configured_for(&self, kind: ProviderKindChoice) -> bool {
        !self.secret_input_value().is_empty()
            || self.environment.configured_for(kind)
            || (self.original_provider_kind == Some(kind) && self.stored_credential_configured)
    }
}

fn reduce_provider_state_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> ProviderModelsCommandOutcome {
    let _ = reduce_settings_command(state, command);
    ProviderModelsCommandOutcome::Handled
}

fn provider_is_usable(summary: &ProviderSummary) -> bool {
    summary.credential_configured && !summary.models.is_empty()
}

fn provider_kind(choice: ProviderKindChoice) -> ProviderKind {
    match choice {
        ProviderKindChoice::Anthropic => ProviderKind::Anthropic,
        ProviderKindChoice::OpenAi => ProviderKind::Openai,
        ProviderKindChoice::Ollama => ProviderKind::Ollama,
    }
}

fn provider_kind_choice(kind: ProviderKind) -> ProviderKindChoice {
    match kind {
        ProviderKind::Anthropic => ProviderKindChoice::Anthropic,
        ProviderKind::Openai => ProviderKindChoice::OpenAi,
        ProviderKind::Ollama => ProviderKindChoice::Ollama,
    }
}

fn synthetic_provider_id(provider: &ProviderConfig) -> String {
    match provider.kind() {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::Openai => "openai",
        ProviderKind::Ollama => "ollama",
    }
    .into()
}

fn nonempty_secret(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}

fn provider_model_ids(entry: &ProviderConfig) -> Vec<String> {
    if entry.models.is_empty() {
        entry.model.iter().cloned().collect()
    } else {
        entry.models.keys().cloned().collect()
    }
}

fn first_model_id(entry: &ProviderConfig) -> Option<String> {
    entry
        .model
        .clone()
        .or_else(|| entry.models.keys().next().cloned())
}

fn ensure_models_are_unambiguous(
    config: &ZodeConfig,
    original_provider_id: Option<&str>,
    draft: &ProviderDraft,
) -> Result<(), String> {
    for (provider_id, provider) in &config.providers {
        if Some(provider_id.as_str()) == original_provider_id {
            continue;
        }
        if let Some(duplicate) = draft
            .model_ids
            .iter()
            .find(|model| provider_model_ids(provider).contains(model))
        {
            return Err(format!("模型 {duplicate} 已属于 Provider {provider_id}"));
        }
    }
    Ok(())
}

fn updated_provider_entry(
    mut entry: ProviderConfig,
    draft: &ProviderDraft,
    kind: ProviderKind,
    api_key: Option<String>,
) -> ProviderConfig {
    let previous_kind = entry.kind();
    let mut overrides = mem::take(&mut entry.models);
    if let Some(legacy_model) = entry.model.take() {
        let legacy_override = ModelOverride {
            context_window: entry.context_window.take(),
            max_output_tokens: entry.max_output_tokens.take(),
            supports_images: entry.supports_images.take(),
            input_price: entry.input_price.take(),
            output_price: entry.output_price.take(),
            cache_read_price: entry.cache_read_price.take(),
            cache_write_price: entry.cache_write_price.take(),
        };
        overrides.entry(legacy_model).or_insert(legacy_override);
    }
    entry.r#type = Some(kind);
    if kind != ProviderKind::Openai || previous_kind != ProviderKind::Openai {
        entry.dialect = None;
    }
    entry.api_key = api_key;
    entry.base_url = draft.base_url.clone();
    entry.models = draft
        .model_ids
        .iter()
        .map(|model_id| {
            (
                model_id.clone(),
                overrides.shift_remove(model_id).unwrap_or_default(),
            )
        })
        .collect();
    entry
}

fn replace_provider_entry(
    config: &mut ZodeConfig,
    original_provider_id: Option<&str>,
    provider_id: String,
    entry: ProviderConfig,
) {
    if original_provider_id == Some(provider_id.as_str()) {
        config.providers.insert(provider_id, entry);
        return;
    }
    let previous = mem::take(&mut config.providers);
    let mut replacement = Some(entry);
    let mut inserted = false;
    for (existing_id, existing) in previous {
        if Some(existing_id.as_str()) == original_provider_id {
            config.providers.insert(
                provider_id.clone(),
                replacement.take().expect("replacement is inserted once"),
            );
            inserted = true;
        } else {
            config.providers.insert(existing_id, existing);
        }
    }
    if !inserted {
        config.providers.insert(
            provider_id,
            replacement.expect("new provider remains available"),
        );
    }
}

trait ProviderHasUserData {
    fn provider_has_user_data(&self) -> bool;
}

impl ProviderHasUserData for ZodeConfig {
    fn provider_has_user_data(&self) -> bool {
        self.provider.model.is_some()
            || self.provider.api_key.is_some()
            || self.provider.base_url.is_some()
            || self.provider.r#type.is_some()
    }
}

#[cfg(test)]
#[path = "provider_models_tests.rs"]
mod persistence_regression_tests;
