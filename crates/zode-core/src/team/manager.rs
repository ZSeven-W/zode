//! TeamManager orchestration: hire / send / dismiss. Dependencies the manager
//! needs from the engine (config snapshot, parent provider, the late-bound
//! parent tool registry, grants, observer, runtime spec) are injected via
//! [`TeamDeps`] at assembly and refreshed on rebuild. Send order is strict:
//! busy-check → claim (atomic, rolled back on any later failure) → dispatch.

use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::file_cache::FileStateCache;
use agent::provider::Provider;
use agent::tool::ToolRegistry;
use agent_tools_code::task::TaskObserver;

use super::ask::{parse_asks, Ask};
use super::board::Board;
use super::external::ExternalSession;
use super::internal::{resolve_teammate_provider, AgentRuntimeSpec, InternalSession};
use super::{claims, persist, tools};
use super::{TeamError, TeamManager, Teammate, TeammateBackend, TeammateStatus};
use crate::config::ZodeConfig;
use crate::external_agents::{ExternalAgentRegistry, GrantStore};

const CLAIM_TTL: Duration = Duration::from_secs(1800);

/// Everything the manager borrows from the engine to build teammates. Cheap
/// to clone (Arcs); refreshed on each engine rebuild so a hot swap of model /
/// sandbox / gate is picked up on the next send.
///
/// Builds an internal teammate's tool registry from `(name, role, tools?)`.
/// `role` selects a default tool set (read-only for reviewer/researcher);
/// `tools`, when present, may only NARROW that default.
pub type BuildInternalToolsFn =
    Arc<dyn Fn(&str, &str, Option<&[String]>) -> Arc<ToolRegistry> + Send + Sync>;
/// Builds a provider from a resolved config (wraps `provider::build_provider`).
pub type BuildProviderFn =
    Arc<dyn Fn(&crate::config::ProviderConfig) -> Result<Arc<dyn Provider>, String> + Send + Sync>;
/// Looks up a user AgentDef by name → `(model?, system)`.
pub type AgentDefLookupFn = Arc<dyn Fn(&str) -> Option<(Option<String>, String)> + Send + Sync>;

#[derive(Clone)]
pub struct TeamDeps {
    pub config: Arc<ZodeConfig>,
    pub parent_provider: crate::config::ProviderConfig,
    pub external_registry: Arc<ExternalAgentRegistry>,
    pub grants: Arc<GrantStore>,
    pub observer: Arc<dyn TaskObserver>,
    pub file_cache: Arc<FileStateCache>,
    pub runtime_spec: AgentRuntimeSpec,
    pub build_internal_tools: BuildInternalToolsFn,
    pub build_provider: BuildProviderFn,
    pub agent_def: AgentDefLookupFn,
    /// Parent approval/permission manager — a teammate is gated by the same
    /// rules as the leader (no bypass).
    pub permissions: Arc<agent::permission::PermissionManager>,
    /// Parent hook runner — edit-history / sandbox / hook blockers apply to
    /// teammates too.
    pub hooks: Arc<agent::hook::HookRunner>,
    pub timeout: Duration,
}

impl std::fmt::Debug for TeamDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamDeps").finish()
    }
}

#[derive(Debug, Clone)]
pub struct HireRequest {
    pub agent: String,
    pub name: String,
    pub role: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tools: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct SendOutcome {
    pub reply: String,
    pub asks: Vec<Ask>,
    pub changed_files: Vec<String>,
    pub warnings: Vec<String>,
}

impl TeamManager {
    fn board(&self) -> Board {
        Board::new(self.team_dir())
    }

    /// Take the exclusive lease if not already held (mutating paths only).
    fn ensure_lease(&self) -> Result<(), TeamError> {
        let mut state = self.state.lock().expect("team state lock");
        if state.lease.is_none() {
            state.lease = Some(super::lease::acquire(&self.team_dir())?);
        }
        Ok(())
    }

    /// Hire a teammate. External: approval-only (grant is pending until the
    /// first send). Internal: build a session immediately. No process runs.
    pub async fn hire(&self, deps: &TeamDeps, req: HireRequest) -> Result<(), TeamError> {
        Self::validate_name(&req.name)?;
        self.ensure_lease()?;
        self.ensure_recovered(deps).await;
        {
            let state = self.state.lock().expect("team state lock");
            if state.teammates.iter().any(|t| t.name == req.name) {
                return Err(TeamError::NameTaken(req.name));
            }
        }

        let (backend, model_label, meta) = if let Some(def) = deps.external_registry.get(&req.agent)
        {
            let session = ExternalSession::new(def.clone())?;
            (
                TeammateBackend::External(Arc::new(tokio::sync::Mutex::new(session))),
                format!("{}(external)", req.agent),
                super::TeammateMeta::External {
                    profile: req.agent.clone(),
                    session_id: None,
                },
            )
        } else {
            // Internal: an AgentDef by this `agent` name contributes its model
            // and system prompt (both overridable by explicit hire args).
            let agent_def = (deps.agent_def)(&req.agent);
            let agent_def_model = agent_def.as_ref().and_then(|(m, _)| m.clone());
            let resolved = resolve_teammate_provider(
                &deps.config,
                req.provider.as_deref(),
                req.model.as_deref(),
                agent_def_model.as_deref(),
                &deps.parent_provider,
            )?;
            let model = resolved
                .model
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let provider = (deps.build_provider)(&resolved).map_err(TeamError::Io)?;
            let roster = self.roster();
            let mut system = super::preamble::render_preamble(
                &req.name,
                &req.role,
                "",
                &roster,
                super::preamble::BoardAccess::Tools,
            );
            if let Some((_, def_system)) = &agent_def {
                if !def_system.trim().is_empty() {
                    system = format!("{def_system}\n\n{system}");
                }
            }
            let session_path = internal_session_path(&self.cwd, &req.name)?;
            let session = InternalSession::new(
                provider,
                model.clone(),
                session_path.clone(),
                system,
                deps.runtime_spec.clone(),
            );
            (
                TeammateBackend::Internal(Arc::new(session)),
                format!("{}(internal, {model})", req.role),
                super::TeammateMeta::Internal {
                    model,
                    session_path,
                },
            )
        };

        {
            let mut state = self.state.lock().expect("team state lock");
            state.teammates.push(Teammate {
                name: req.name,
                role: req.role,
                backend,
                status: TeammateStatus::Idle,
                generation: 0,
                usage_in: 0,
                usage_out: 0,
                model_label,
                allowed_tools: req.tools,
                meta,
            });
        }
        self.persist();
        Ok(())
    }

    /// Dispatch a task to a teammate. Strict order: busy-check → claim → run;
    /// a failure after claiming rolls the claims back.
    pub async fn send(
        &self,
        deps: &TeamDeps,
        to: &str,
        message: &str,
        claim_paths: &[String],
        abort: AbortController,
    ) -> Result<SendOutcome, TeamError> {
        self.ensure_recovered(deps).await;
        // 1. busy-check + bump generation (before any claim).
        let generation = {
            let mut state = self.state.lock().expect("team state lock");
            let t = state
                .teammates
                .iter_mut()
                .find(|t| t.name == to)
                .ok_or_else(|| TeamError::UnknownTeammate(to.to_string()))?;
            if let TeammateStatus::Busy { desc, .. } = &t.status {
                return Err(TeamError::Busy { desc: desc.clone() });
            }
            t.generation += 1;
            let g = t.generation;
            t.status = TeammateStatus::Busy {
                desc: message.chars().take(60).collect(),
                generation: g,
            };
            g
        };

        // 2. claim (atomic; rolled back if dispatch setup fails).
        if !claim_paths.is_empty() {
            let board = self.board();
            if let Err(c) = claims::claim(
                &board,
                to,
                claim_paths,
                &self.cwd,
                CLAIM_TTL,
                tools::now_ms(),
            ) {
                self.reset_idle(to, generation);
                return Err(TeamError::Io(format!(
                    "claim conflict: {}",
                    serde_json::to_string(&c.conflicts).unwrap_or_default()
                )));
            }
        }

        // 3. dispatch. A renewal task extends the claims' TTL while the send
        // is in flight so a long task can't lose its reservation mid-run.
        let obs_id = deps.observer.on_start(to, Some(message), 1);
        let roster_names: Vec<String> = self.roster().into_iter().map(|t| t.name).collect();
        let renewal = if claim_paths.is_empty() {
            None
        } else {
            let board = self.board();
            let holder = to.to_string();
            Some(tokio::spawn(async move {
                let interval = CLAIM_TTL / 3;
                loop {
                    tokio::time::sleep(interval).await;
                    claims::renew(&board, &holder, CLAIM_TTL, tools::now_ms());
                }
            }))
        };
        let result = self.dispatch(deps, to, message, obs_id, abort).await;
        if let Some(task) = renewal {
            task.abort();
        }

        // Release claims + settle status guarded by generation (a stale
        // completion must not clobber a newer send).
        if !claim_paths.is_empty() {
            claims::release(&self.board(), to, Some(claim_paths));
        }
        self.reset_idle(to, generation);

        match result {
            Ok((reply, changed_files, usage)) => {
                self.add_usage(to, usage.0, usage.1);
                // Persist so a captured external session id / usage survives a
                // restart.
                self.persist();
                let (asks, warnings) = parse_asks(&reply, &roster_names, to);
                Ok(SendOutcome {
                    reply,
                    asks,
                    changed_files,
                    warnings,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Backend-specific execution. Clones the backend Arc out from under the
    /// roster lock, then drives it WITHOUT holding the lock across `.await`.
    /// Returns (reply, changed_files, (in,out)).
    async fn dispatch(
        &self,
        deps: &TeamDeps,
        to: &str,
        message: &str,
        obs_id: u64,
        abort: AbortController,
    ) -> Result<(String, Vec<String>, (u64, u64)), TeamError> {
        let (backend, role, allowed_tools) = {
            let state = self.state.lock().expect("team state lock");
            state
                .teammates
                .iter()
                .find(|t| t.name == to)
                .map(|t| (t.backend.clone(), t.role.clone(), t.allowed_tools.clone()))
                .ok_or_else(|| TeamError::UnknownTeammate(to.to_string()))?
        };
        match backend {
            TeammateBackend::Internal(session) => {
                let tools = (deps.build_internal_tools)(to, &role, allowed_tools.as_deref());
                let env = super::internal::SendEnv {
                    tools,
                    permissions: deps.permissions.clone(),
                    hooks: deps.hooks.clone(),
                    file_cache: deps.file_cache.clone(),
                };
                let out = session
                    .send(message, env, deps.observer.clone(), obs_id, abort)
                    .await?;
                Ok((out.reply, Vec::new(), (out.usage_in, out.usage_out)))
            }
            TeammateBackend::External(session) => {
                let mut guard = session.lock().await;
                let roster = self.roster();
                let preamble = if guard.session_id.is_none() {
                    Some(super::preamble::render_preamble(
                        to,
                        &roster
                            .iter()
                            .find(|r| r.name == to)
                            .map(|r| r.role.clone())
                            .unwrap_or_default(),
                        "",
                        &roster,
                        super::preamble::BoardAccess::Inline,
                    ))
                } else {
                    None
                };
                let observer = deps.observer.clone();
                let out = guard
                    .send(
                        message,
                        preamble.as_deref(),
                        &deps.grants,
                        &self.cwd,
                        deps.timeout,
                        Some(deps.file_cache.clone()),
                        move |ev| {
                            let event = match ev {
                                crate::external_agents::parser::ExtEvent::Text(delta) => {
                                    agent::stream::Event::TextDelta { delta }
                                }
                                crate::external_agents::parser::ExtEvent::ToolUse {
                                    name,
                                    summary,
                                } => agent::stream::Event::ToolUse {
                                    id: String::new(),
                                    name,
                                    input: serde_json::json!({ "summary": summary }),
                                },
                                crate::external_agents::parser::ExtEvent::Log(message) => {
                                    agent::stream::Event::Notice {
                                        code: "external-log".into(),
                                        message,
                                    }
                                }
                            };
                            observer.on_event(obs_id, &event);
                        },
                        abort,
                    )
                    .await?;
                let usage = (
                    out.result.usage_in.unwrap_or(0),
                    out.result.usage_out.unwrap_or(0),
                );
                // Mirror the (possibly newly-captured) session id onto the
                // roster so `persist` can record it without locking the async
                // session mutex.
                let new_sid = guard.session_id.clone();
                drop(guard);
                if let Some(sid) = new_sid {
                    let mut state = self.state.lock().expect("team state lock");
                    if let Some(t) = state.teammates.iter_mut().find(|t| t.name == to) {
                        if let super::TeammateMeta::External { session_id, .. } = &mut t.meta {
                            *session_id = Some(sid);
                        }
                    }
                }
                deps.observer.on_finish(obs_id, &out.result.text, None);
                Ok((out.result.text, out.changed_files, usage))
            }
        }
    }

    /// Serialize the current roster to `team.json` under the HMAC key.
    /// Best-effort: persistence failures are logged, not surfaced.
    fn persist(&self) {
        let key = match persist::load_or_create_key() {
            Ok(k) => k,
            Err(_) => return,
        };
        let roster: Vec<persist::PersistedTeammate> = {
            let state = self.state.lock().expect("team state lock");
            state
                .teammates
                .iter()
                .map(|t| match &t.meta {
                    super::TeammateMeta::Internal {
                        model,
                        session_path,
                    } => persist::PersistedTeammate {
                        name: t.name.clone(),
                        role: t.role.clone(),
                        kind: "internal".into(),
                        model_label: t.model_label.clone(),
                        session_id: None,
                        profile: None,
                        fingerprint_hash: Some(model.clone()),
                        session_path: Some(session_path.to_string_lossy().to_string()),
                    },
                    super::TeammateMeta::External {
                        profile,
                        session_id,
                    } => persist::PersistedTeammate {
                        name: t.name.clone(),
                        role: t.role.clone(),
                        kind: "external".into(),
                        model_label: t.model_label.clone(),
                        session_id: session_id.clone(),
                        profile: Some(profile.clone()),
                        fingerprint_hash: None,
                        session_path: None,
                    },
                })
                .collect()
        };
        let board_revision = self.board().read().map(|s| s.revision).unwrap_or(0);
        let state_file = persist::TeamStateFile {
            schema: 1,
            roster,
            board_revision,
        };
        let _ = persist::save(&state_file, &self.team_dir(), &key);
    }

    /// Recover a persisted roster once per manager, on the first mutating op.
    /// Internal teammates rebuild their provider + reload history; external
    /// teammates restore the session id but require a fresh trust approval
    /// on their next send (the grant is never persisted). A quarantined /
    /// unreadable file leaves the roster empty.
    async fn ensure_recovered(&self, deps: &TeamDeps) {
        {
            let mut state = self.state.lock().expect("team state lock");
            if state.recovered {
                return;
            }
            state.recovered = true;
        }
        let key = match persist::load_or_create_key() {
            Ok(k) => k,
            Err(_) => return,
        };
        let loaded = match persist::load(&self.team_dir(), &key) {
            Ok(Some(f)) => f,
            _ => return, // no file, or quarantined → start empty
        };
        for pt in loaded.roster {
            // Skip a name that already exists (idempotent recovery).
            if self.roster().iter().any(|r| r.name == pt.name) {
                continue;
            }
            let rebuilt = if pt.kind == "external" {
                pt.profile
                    .as_ref()
                    .and_then(|p| deps.external_registry.get(p).cloned())
                    .and_then(|def| ExternalSession::new(def).ok())
                    .map(|mut s| {
                        let supports_resume = s.def.capability.resume_flag.is_some()
                            || s.def.capability.resume_args.is_some();
                        let restored_session_id = if supports_resume {
                            pt.session_id.clone()
                        } else {
                            None
                        };
                        s.session_id = restored_session_id.clone();
                        (
                            TeammateBackend::External(Arc::new(tokio::sync::Mutex::new(s))),
                            pt.model_label.clone(),
                            super::TeammateMeta::External {
                                profile: pt.profile.clone().unwrap_or_default(),
                                session_id: restored_session_id,
                            },
                        )
                    })
            } else {
                let model = pt.fingerprint_hash.clone().unwrap_or_default();
                let resolved = resolve_teammate_provider(
                    &deps.config,
                    None,
                    Some(&model),
                    None,
                    &deps.parent_provider,
                )
                .ok();
                match resolved.and_then(|r| (deps.build_provider)(&r).ok()) {
                    Some(provider) => {
                        let session_path = pt
                            .session_path
                            .clone()
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| {
                                internal_session_path(&self.cwd, &pt.name).unwrap_or_default()
                            });
                        let system = super::preamble::render_preamble(
                            &pt.name,
                            &pt.role,
                            "",
                            &self.roster(),
                            super::preamble::BoardAccess::Tools,
                        );
                        let session = InternalSession::new(
                            provider,
                            model.clone(),
                            session_path.clone(),
                            system,
                            deps.runtime_spec.clone(),
                        );
                        session.restore_history().await;
                        Some((
                            TeammateBackend::Internal(Arc::new(session)),
                            pt.model_label.clone(),
                            super::TeammateMeta::Internal {
                                model,
                                session_path,
                            },
                        ))
                    }
                    None => None,
                }
            };
            if let Some((backend, model_label, meta)) = rebuilt {
                let mut state = self.state.lock().expect("team state lock");
                state.teammates.push(Teammate {
                    name: pt.name,
                    role: pt.role,
                    backend,
                    status: TeammateStatus::Idle,
                    generation: 0,
                    usage_in: 0,
                    usage_out: 0,
                    model_label,
                    allowed_tools: None,
                    meta,
                });
            }
        }
    }

    fn reset_idle(&self, to: &str, generation: u64) {
        let mut state = self.state.lock().expect("team state lock");
        if let Some(t) = state.teammates.iter_mut().find(|t| t.name == to) {
            // Only clear if this is still the current generation — a late
            // completion from an older send must not reset a newer Busy.
            if t.generation == generation {
                t.status = TeammateStatus::Idle;
            }
        }
    }

    fn add_usage(&self, to: &str, input: u64, output: u64) {
        let mut state = self.state.lock().expect("team state lock");
        if let Some(t) = state.teammates.iter_mut().find(|t| t.name == to) {
            t.usage_in += input;
            t.usage_out += output;
        }
    }

    /// Dismiss a teammate: release its claims and remove it from the roster.
    pub async fn dismiss(&self, name: &str) -> Result<(), TeamError> {
        claims::release(&self.board(), name, None);
        {
            let mut state = self.state.lock().expect("team state lock");
            let before = state.teammates.len();
            state.teammates.retain(|t| t.name != name);
            if state.teammates.len() == before {
                return Err(TeamError::UnknownTeammate(name.to_string()));
            }
        }
        self.persist();
        Ok(())
    }
}

/// Path for an internal teammate's persisted history.
fn internal_session_path(
    cwd: &std::path::Path,
    name: &str,
) -> Result<std::path::PathBuf, TeamError> {
    let root = agent::session::Session::zode_root()
        .ok_or_else(|| TeamError::Io("no session root".to_string()))?;
    // Namespace by a stable hash of the workspace so different repos don't
    // collide on teammate names.
    let tag = format!("{:x}", md5_like(cwd.to_string_lossy().as_bytes()));
    Ok(root.join("team").join(tag).join(format!("{name}.jsonl")))
}

/// Tiny non-crypto path tag (FNV-1a). Not security-sensitive — just avoids
/// cross-repo teammate-name collisions in the session directory.
fn md5_like(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExternalAgentsConfig;
    use crate::external_agents::discover;
    use crate::subagents::SubAgentRegistry;
    use std::path::Path;

    fn ext_registry(name: &str, script: &str) -> Arc<ExternalAgentRegistry> {
        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            name.to_string(),
            serde_json::from_value(serde_json::json!({
                "command": Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/extagent").join(script)
                    .display().to_string(),
                "args": [],
                "promptTransport": "stdin",
                "output": "jsonl-claude",
                "resumeFlag": "--resume",
                "trusted": true,
            }))
            .unwrap(),
        );
        Arc::new(discover(&cfg, &[]))
    }

    fn deps(reg: Arc<ExternalAgentRegistry>) -> (TeamDeps, Arc<GrantStore>) {
        let grants = Arc::new(GrantStore::default());
        // Pre-grant the external profile so send doesn't need an interactive gate.
        if let Some(def) = reg.get("my-ext") {
            let cwd = std::env::temp_dir();
            let mut fp = crate::external_agents::preapproval_fingerprint(def, &cwd).unwrap();
            fp.version_output = Some("1.0".into());
            grants.store_pending(&def.name, fp);
            grants.promote(&def.name, "1.0".into());
        }
        let d = TeamDeps {
            config: Arc::new(ZodeConfig::default()),
            parent_provider: crate::config::ProviderConfig::default(),
            external_registry: reg,
            grants: grants.clone(),
            observer: SubAgentRegistry::new().observer(),
            file_cache: Arc::new(FileStateCache::new(
                std::num::NonZeroUsize::new(8).unwrap(),
                1 << 20,
            )),
            runtime_spec: AgentRuntimeSpec::default(),
            build_internal_tools: Arc::new(|_, _, _| Arc::new(ToolRegistry::new())),
            build_provider: Arc::new(|_| Err("no provider in test".into())),
            agent_def: Arc::new(|_| None),
            permissions: Arc::new(agent::permission::PermissionManager::new()),
            hooks: Arc::new(agent::hook::HookRunner::new()),
            timeout: Duration::from_secs(30),
        };
        (d, grants)
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn hire_and_send_external_teammate() {
        let dir = tempfile::tempdir().unwrap();
        // Grant must be keyed to the SEND cwd (the manager's cwd), not temp.
        let reg = ext_registry("my-ext", "fake-resume.sh");
        let grants = Arc::new(GrantStore::default());
        if let Some(def) = reg.get("my-ext") {
            let mut fp = crate::external_agents::preapproval_fingerprint(def, dir.path()).unwrap();
            fp.version_output = Some("1.0".into());
            grants.store_pending(&def.name, fp);
            grants.promote(&def.name, "1.0".into());
        }
        let (mut d, _) = deps(reg);
        d.grants = grants;

        let mgr = TeamManager::new(dir.path().to_path_buf());
        mgr.hire(
            &d,
            HireRequest {
                agent: "my-ext".into(),
                name: "codex-impl".into(),
                role: "builder".into(),
                provider: None,
                model: None,
                tools: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(mgr.roster().len(), 1);
        assert_eq!(mgr.roster()[0].model_label, "my-ext(external)");

        let out = mgr
            .send(&d, "codex-impl", "task one", &[], AbortController::new())
            .await
            .unwrap();
        assert!(out.reply.contains("first-run done") || out.reply.contains("resumed-ok"));
        // roster returns to idle after send
        assert_eq!(mgr.roster()[0].status_line, "idle");

        // dismiss removes it
        mgr.dismiss("codex-impl").await.unwrap();
        assert!(mgr.roster().is_empty());
        assert!(mgr.dismiss("codex-impl").await.is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn external_teammate_survives_restart_via_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let reg = ext_registry("my-ext", "fake-resume.sh");
        let grants = Arc::new(GrantStore::default());
        if let Some(def) = reg.get("my-ext") {
            let mut fp = crate::external_agents::preapproval_fingerprint(def, dir.path()).unwrap();
            fp.version_output = Some("1.0".into());
            grants.store_pending(&def.name, fp);
            grants.promote(&def.name, "1.0".into());
        }
        let (mut d, _) = deps(reg);
        d.grants = grants;

        // First manager: hire + send (captures session id), then persist.
        {
            let mgr = TeamManager::new(dir.path().to_path_buf());
            mgr.hire(
                &d,
                HireRequest {
                    agent: "my-ext".into(),
                    name: "codex-impl".into(),
                    role: "builder".into(),
                    provider: None,
                    model: None,
                    tools: None,
                },
            )
            .await
            .unwrap();
            mgr.send(&d, "codex-impl", "task one", &[], AbortController::new())
                .await
                .unwrap();
            mgr.shutdown().await;
        }
        // Second manager (simulated restart): recover from team.json.
        let mgr2 = TeamManager::new(dir.path().to_path_buf());
        // ensure_recovered runs on the first mutating op; trigger via a send.
        let out = mgr2
            .send(&d, "codex-impl", "task two", &[], AbortController::new())
            .await;
        // The teammate was recovered (send resolved a known teammate, not
        // UnknownTeammate).
        assert!(
            !matches!(out, Err(TeamError::UnknownTeammate(_))),
            "recovered teammate should be known: {out:?}"
        );
        assert_eq!(mgr2.roster().len(), 1);
        assert_eq!(mgr2.roster()[0].name, "codex-impl");
    }

    #[tokio::test]
    async fn hire_rejects_duplicate_and_bad_name() {
        let dir = tempfile::tempdir().unwrap();
        let (d, _) = deps(ext_registry("my-ext", "fake-resume.sh"));
        let mgr = TeamManager::new(dir.path().to_path_buf());
        assert!(matches!(
            mgr.hire(
                &d,
                HireRequest {
                    agent: "my-ext".into(),
                    name: "Bad Name".into(),
                    role: "x".into(),
                    provider: None,
                    model: None,
                    tools: None,
                },
            )
            .await,
            Err(TeamError::NameInvalid(_))
        ));
    }

    #[test]
    fn late_completion_does_not_clobber_new_generation() {
        // reset_idle only clears the status when its generation still matches
        // — a stale send's completion must not reset a newer Busy.
        let dir = tempfile::tempdir().unwrap();
        let mgr = TeamManager::new(dir.path().to_path_buf());
        {
            let mut state = mgr.state.lock().unwrap();
            state.teammates.push(Teammate {
                name: "w".into(),
                role: "r".into(),
                backend: TeammateBackend::External(Arc::new(tokio::sync::Mutex::new(
                    ExternalSession::new(crate::external_agents::ExternalAgentDef {
                        name: "x".into(),
                        command: "/bin/true".into(),
                        args: vec![],
                        capability: crate::external_agents::capability::ProfileCapability {
                            prompt_transport:
                                crate::external_agents::capability::PromptTransport::Stdin,
                            output_protocol:
                                crate::external_agents::capability::OutputProtocol::Text,
                            resume_flag: Some("--resume".into()),
                            resume_args: None,
                            new_session_args: None,
                            effective_sandbox:
                                crate::external_agents::capability::EffectiveSandbox::Unknown,
                            version_requirement: None,
                            session_id_source: None,
                            text_source: None,
                        },
                        auth_env: vec![],
                        env_allow: vec![],
                        trusted: false,
                    })
                    .unwrap(),
                ))),
                status: TeammateStatus::Busy {
                    desc: "gen2".into(),
                    generation: 2,
                },
                generation: 2,
                usage_in: 0,
                usage_out: 0,
                model_label: "x".into(),
                allowed_tools: None,
                meta: crate::team::TeammateMeta::External {
                    profile: "x".into(),
                    session_id: None,
                },
            });
        }
        // An older send (gen 1) completing must NOT reset the gen-2 Busy.
        mgr.reset_idle("w", 1);
        assert_eq!(mgr.roster()[0].status_line, "busy: gen2");
        // The current generation's completion does reset it.
        mgr.reset_idle("w", 2);
        assert_eq!(mgr.roster()[0].status_line, "idle");
    }

    #[tokio::test]
    async fn send_rolls_back_claims_when_teammate_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        let (d, _) = deps(ext_registry("my-ext", "fake-resume.sh"));
        let mgr = TeamManager::new(cwd.clone());
        // send to a nonexistent teammate → error, no claim leaked
        assert!(mgr
            .send(
                &d,
                "ghost",
                "hi",
                &["src/a.rs".into()],
                AbortController::new()
            )
            .await
            .is_err());
        let board = Board::new(cwd.join(".zode/team"));
        assert!(board.read().map(|s| s.claims.is_empty()).unwrap_or(true));
    }
}
