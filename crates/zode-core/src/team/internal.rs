//! Internal teammate sessions: an in-process persistent conversation driven
//! by a fresh QueryLoop per send, over a shared `MessageStore`. The single
//! driver invariant is respected — we never hand-push messages; QueryLoop
//! owns the store while it runs. A per-teammate run mutex serializes sends
//! (concurrent send to the same teammate → Busy, not a queue).

use std::path::PathBuf;
use std::sync::Arc;

use agent::abort::AbortController;
use agent::hook::HookRunner;
use agent::message::MessageStore;
use agent::permission::PermissionManager;
use agent::provider::Provider;
use agent::query::QueryLoop;
use agent::tool::ToolRegistry;
use agent_tools_code::task::TaskObserver;
use tokio::sync::Mutex as AsyncMutex;

use super::TeamError;
use crate::config::{ProviderConfig, ZodeConfig};

/// The QueryLoop knobs shared between the main engine loop and internal
/// teammates. Extracted so a teammate builds a full-fidelity loop instead of
/// silently falling back to builder defaults (spec §4.3).
#[derive(Debug, Clone)]
pub struct AgentRuntimeSpec {
    pub max_output_tokens: u32,
    pub model_max_tokens: u32,
    pub max_iterations: usize,
    pub max_api_retries: u32,
    pub temperature: Option<f32>,
    pub use_prompt_cache: bool,
    pub microcompact: bool,
}

impl Default for AgentRuntimeSpec {
    fn default() -> Self {
        Self {
            max_output_tokens: 8192,
            model_max_tokens: 200_000,
            max_iterations: 0,
            max_api_retries: 10,
            temperature: None,
            use_prompt_cache: true,
            microcompact: false,
        }
    }
}

/// Resolve a teammate's provider+model. Precedence: hire args, then
/// AgentDef.model, then the parent snapshot. With both provider and model,
/// uses the paired resolver (correct when several providers expose the same
/// model id).
pub fn resolve_teammate_provider(
    cfg: &ZodeConfig,
    provider: Option<&str>,
    model: Option<&str>,
    agent_def_model: Option<&str>,
    parent: &ProviderConfig,
) -> Result<ProviderConfig, TeamError> {
    let effective_model = model.or(agent_def_model);
    match (provider, effective_model) {
        (Some(p), Some(m)) => cfg
            .resolve_named_provider_model(p, m)
            .ok_or_else(|| TeamError::Io(format!("no provider '{p}' with model '{m}'"))),
        (Some(p), None) => cfg
            .resolve_named_provider(p)
            .ok_or_else(|| TeamError::Io(format!("unknown provider '{p}'"))),
        (None, Some(m)) => cfg
            .resolve_model_provider(m)
            .ok_or_else(|| TeamError::Io(format!("no configured provider owns model '{m}'"))),
        (None, None) => Ok(parent.clone()),
    }
}

#[derive(Debug)]
pub struct InternalSession {
    provider: Arc<dyn Provider>,
    model: String,
    store: Arc<std::sync::Mutex<MessageStore>>,
    session_path: PathBuf,
    system: String,
    spec: AgentRuntimeSpec,
    run_lock: Arc<AsyncMutex<()>>,
}

impl InternalSession {
    pub fn new(
        provider: Arc<dyn Provider>,
        model: String,
        session_path: PathBuf,
        system: String,
        spec: AgentRuntimeSpec,
    ) -> Self {
        Self {
            provider,
            model,
            store: Arc::new(std::sync::Mutex::new(MessageStore::new())),
            session_path,
            system,
            spec,
            run_lock: Arc::new(AsyncMutex::new(())),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn session_path(&self) -> &PathBuf {
        &self.session_path
    }

    /// Rehydrate the conversation history from disk (recovery after restart).
    /// Best-effort: a missing / unreadable file leaves an empty store.
    pub async fn restore_history(&self) {
        if let Ok(store) = agent::session::Session::load(&self.session_path).await {
            if let Ok(mut guard) = self.store.lock() {
                *guard = store;
            }
        }
    }

    /// Drive one send to completion. Concurrent send → `Busy`. Rebuilds a
    /// QueryLoop from CURRENT tools each call (so an engine rebuild's new
    /// sandbox/gate is picked up), aggregates the reply text and usage, then
    /// persists. `env.permissions`/`env.hooks` are the PARENT's, so a teammate
    /// is bound by the same approval gate, sandbox, and edit-history hooks.
    pub async fn send(
        &self,
        message: &str,
        env: SendEnv,
        observer: Arc<dyn TaskObserver>,
        obs_id: u64,
        abort: AbortController,
    ) -> Result<SendReply, TeamError> {
        let SendEnv {
            tools,
            permissions,
            hooks,
            file_cache,
        } = env;
        let Ok(_run) = self.run_lock.try_lock() else {
            return Err(TeamError::Busy {
                desc: "already handling a task".to_string(),
            });
        };
        let mut builder = QueryLoop::builder(self.provider.clone(), self.model.clone())
            .tools(tools)
            .permissions(permissions)
            .hooks(hooks)
            .store(self.store.clone())
            .file_cache(file_cache)
            .max_output_tokens(self.spec.max_output_tokens)
            .model_max_tokens(self.spec.model_max_tokens)
            .max_iterations(self.spec.max_iterations)
            .max_api_retries(self.spec.max_api_retries)
            .auto_compact(true)
            .microcompact(self.spec.microcompact)
            .use_prompt_cache(self.spec.use_prompt_cache)
            .system(self.system.clone());
        if let Some(t) = self.spec.temperature {
            builder = builder.temperature(t);
        }

        let mut stream = builder.build().run(message, abort).await.map_err(|e| {
            observer.on_finish(obs_id, "", Some(&e.to_string()));
            TeamError::Io(e.to_string())
        })?;

        use agent::stream::Event;
        use futures::StreamExt;
        let mut reply = String::new();
        let mut error: Option<String> = None;
        // Usage frames are cumulative within a turn and reset between turns —
        // settle the peak per turn (same algorithm as SubAgentRegistry).
        let (mut committed_in, mut committed_out) = (0u64, 0u64);
        let (mut turn_in, mut turn_out) = (0u64, 0u64);
        while let Some(ev) = stream.next().await {
            if let Ok(ev) = &ev {
                observer.on_event(obs_id, ev);
            }
            match ev {
                Ok(Event::TextDelta { delta }) => reply.push_str(&delta),
                Ok(Event::Usage {
                    input_tokens,
                    output_tokens,
                    ..
                }) => {
                    if (input_tokens as u64) < turn_in || (output_tokens as u64) < turn_out {
                        committed_in += turn_in;
                        committed_out += turn_out;
                        turn_in = 0;
                        turn_out = 0;
                    }
                    turn_in = turn_in.max(input_tokens as u64);
                    turn_out = turn_out.max(output_tokens as u64);
                }
                Ok(Event::Error { message, .. }) => error = Some(message),
                Err(e) => error = Some(e.to_string()),
                _ => {}
            }
        }
        observer.on_finish(obs_id, &reply, error.as_deref());

        // Persist: clone the store under the std lock, then await the write
        // with the lock released (mirrors the engine's session-save pattern).
        let snapshot = {
            let store = self.store.lock().expect("store lock");
            store.clone()
        };
        let path = self.session_path.clone();
        let _ = agent::session::Session::save(&path, &snapshot).await;

        if let Some(e) = error {
            return Err(TeamError::Io(e));
        }
        Ok(SendReply {
            reply,
            usage_in: committed_in + turn_in,
            usage_out: committed_out + turn_out,
        })
    }
}

/// Result of an internal teammate send.
#[derive(Debug)]
pub struct SendReply {
    pub reply: String,
    pub usage_in: u64,
    pub usage_out: u64,
}

/// Per-send execution environment: the teammate's (role-filtered) tools plus
/// the PARENT's permission manager, hooks, and file cache — so a teammate is
/// gated and tracked exactly like the leader.
pub struct SendEnv {
    pub tools: Arc<ToolRegistry>,
    pub permissions: Arc<PermissionManager>,
    pub hooks: Arc<HookRunner>,
    pub file_cache: Arc<agent::file_cache::FileStateCache>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind};
    use indexmap::IndexMap;

    fn cfg_two_providers() -> ZodeConfig {
        let mut cfg = ZodeConfig::default();
        let mut main = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("k1".into()),
            ..Default::default()
        };
        main.models = IndexMap::new();
        main.models.insert("model-a".into(), Default::default());
        let mut cheap = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("k2".into()),
            ..Default::default()
        };
        cheap.models = IndexMap::new();
        cheap.models.insert("model-b".into(), Default::default());
        cfg.providers.insert("main".into(), main);
        cfg.providers.insert("cheap".into(), cheap);
        cfg
    }

    #[test]
    fn provider_resolution_precedence() {
        let cfg = cfg_two_providers();
        let parent = ProviderConfig {
            r#type: Some(ProviderKind::Anthropic),
            api_key: Some("parent".into()),
            model: Some("parent-model".into()),
            ..Default::default()
        };
        // hire args beat AgentDef.model
        let r = resolve_teammate_provider(
            &cfg,
            Some("cheap"),
            Some("model-b"),
            Some("model-a"),
            &parent,
        )
        .unwrap();
        assert_eq!(r.model.as_deref(), Some("model-b"));
        assert_eq!(r.api_key.as_deref(), Some("k2"));
        // AgentDef.model beats parent when no hire override
        let r = resolve_teammate_provider(&cfg, None, None, Some("model-a"), &parent).unwrap();
        assert_eq!(r.model.as_deref(), Some("model-a"));
        // nothing specified → parent snapshot
        let r = resolve_teammate_provider(&cfg, None, None, None, &parent).unwrap();
        assert_eq!(r.model.as_deref(), Some("parent-model"));
        // unknown provider errors
        assert!(resolve_teammate_provider(&cfg, Some("nope"), None, None, &parent).is_err());
    }
}
