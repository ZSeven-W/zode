use std::sync::Arc;

use zode_core::config::{ConfigManager, ProviderKind};
use zode_node_protocol::{EndpointError, SessionLocator};

use super::{lock, map_internal, unavailable, RuntimeSession, SessionSlot, ZodeEngineDriver};

struct StagedSessionReload {
    session: SessionLocator,
    slot: Arc<SessionSlot>,
    runtime: RuntimeSession,
    persisted_model: String,
    target_model: String,
}

impl ZodeEngineDriver {
    pub(super) async fn reload_provider_configuration(&self) -> Result<(), EndpointError> {
        let config_dir = self
            .config_dir
            .as_deref()
            .ok_or_else(|| unavailable("provider configuration directory is unavailable"))?;
        let mut config = ConfigManager::load_global_in(config_dir).map_err(map_internal)?;
        config.normalize_legacy();
        for provider in config.providers.values_mut() {
            match provider.kind() {
                ProviderKind::Ollama => fill_from_env(&mut provider.base_url, "OLLAMA_HOST"),
                ProviderKind::Anthropic => {
                    fill_from_env(&mut provider.api_key, "ANTHROPIC_API_KEY")
                }
                ProviderKind::Openai => fill_from_env(&mut provider.api_key, "OPENAI_API_KEY"),
            }
        }
        config.resolve_provider_from_map();
        config.apply_env_fallbacks();
        let providers = config.providers;
        let active_provider = config.provider;

        let refreshed_base = lock(&self.template)
            .clone()
            .with_providers_map(providers.clone())
            .with_provider_config(active_provider.clone());
        let refreshed_models = refreshed_base.model_ids();
        let mut loaded_sessions = lock(&self.sessions)
            .iter()
            .filter(|(_, slot)| lock(&slot.runtime).is_some())
            .map(|(session, slot)| (session.clone(), slot.clone()))
            .collect::<Vec<_>>();
        loaded_sessions.sort_by(|(left, _), (right, _)| left.cmp(right));

        // Lock every loaded session in a stable order. This prevents a turn or
        // another settings mutation from changing a snapshot while the full
        // reload transaction is being prepared.
        let mut mutation_guards = Vec::with_capacity(loaded_sessions.len());
        for (_, slot) in &loaded_sessions {
            mutation_guards.push(slot.mutation.lock().await);
        }

        // Assemble every replacement before publishing any of them. If one
        // provider is invalid or one engine assembly fails, this function exits
        // with the old base template and every old session engine intact.
        let mut staged = Vec::with_capacity(loaded_sessions.len());
        for (session, slot) in &loaded_sessions {
            let Some((old_engine, old_template, persisted_messages)) =
                lock(&slot.runtime).as_ref().map(|runtime| {
                    (
                        runtime.engine.clone(),
                        runtime.template.clone(),
                        runtime.persisted_messages,
                    )
                })
            else {
                continue;
            };
            let snapshot = old_engine.snapshot().await?;
            let target_model = snapshot
                .model
                .clone()
                .into_valid_model(&refreshed_models)
                .or_else(|| active_provider.model.clone())
                .unwrap_or(snapshot.model.clone());
            let template = old_template
                .with_providers_map(providers.clone())
                .with_provider_config(active_provider.clone())
                .with_model(target_model.clone());
            let mut loaded = self.repository.load(session).await?;
            let persisted_model = loaded.meta.model.clone();
            loaded.store = snapshot.store;
            loaded.meta.model = target_model.clone();
            let engine = self
                .factory
                .assemble(&template, session, loaded, Some(snapshot.carry))
                .await?;
            staged.push(StagedSessionReload {
                session: session.clone(),
                slot: slot.clone(),
                runtime: RuntimeSession {
                    engine,
                    template,
                    persisted_messages,
                },
                persisted_model,
                target_model,
            });
        }

        persist_model_changes(&self.repository, &staged).await?;

        // Hold all runtime locks while publishing the base template and every
        // replacement. Readers therefore observe either the complete old set or
        // the complete refreshed set, never a partially reloaded combination.
        let commit_slots = staged
            .iter()
            .map(|reload| reload.slot.clone())
            .collect::<Vec<_>>();
        let mut runtime_guards = commit_slots
            .iter()
            .map(|slot| lock(&slot.runtime))
            .collect::<Vec<_>>();
        *lock(&self.template) = refreshed_base;
        for (guard, reload) in runtime_guards.iter_mut().zip(staged) {
            **guard = Some(reload.runtime);
        }

        drop(mutation_guards);
        Ok(())
    }
}

trait ValidModel {
    fn into_valid_model(self, available: &[String]) -> Option<String>;
}

impl ValidModel for String {
    fn into_valid_model(self, available: &[String]) -> Option<String> {
        available.iter().any(|model| model == &self).then_some(self)
    }
}

async fn persist_model_changes(
    repository: &crate::LocalSessionRepository,
    staged: &[StagedSessionReload],
) -> Result<(), EndpointError> {
    let mut committed = Vec::new();
    for reload in staged {
        if reload.persisted_model == reload.target_model {
            continue;
        }
        if let Err(error) = repository
            .update_model(&reload.session, reload.target_model.clone())
            .await
        {
            for (session, old_model) in committed.into_iter().rev() {
                let _ = repository.update_model(&session, old_model).await;
            }
            return Err(error);
        }
        committed.push((reload.session.clone(), reload.persisted_model.clone()));
    }
    Ok(())
}

fn fill_from_env(target: &mut Option<String>, name: &str) {
    if target.is_some() {
        return;
    }
    if let Ok(value) = std::env::var(name) {
        if !value.is_empty() {
            *target = Some(value);
        }
    }
}
