//! `ZodeTaskTool` — the self-gated Task router (spec v2.3 ADR-4/-5).
//!
//! Internal `agent_type`s pass through to the upstream `TaskTool` verbatim
//! (the outer permission wrapper still gates that path exactly as before —
//! this decorator is registered UNWRAPPED only when external agents exist,
//! and then reproduces the internal gate itself). External `agent_type`s run
//! a trust-delegated CLI process: fingerprint check → (approval) → re-hash →
//! version probe → spawn. One approval per call, never two.

use std::sync::Arc;
use std::time::Duration;

use agent::error::AgentError;
use agent::file_cache::FileStateCache;
use agent::stream::Event;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use agent_tools_code::task::{TaskFinishGuard, TaskObserver, DEFAULT_MAX_DEPTH};
use async_trait::async_trait;

use crate::approval::{Approval, ApprovalGate, ApprovalScope};
use crate::external_agents::runner::{run_external, RunSpec};
use crate::external_agents::{
    limiter::ExternalLimiter, parser::ExtEvent, preapproval_fingerprint, ExternalAgentDef,
    ExternalAgentRegistry, Fingerprint, GrantCheck, GrantStore,
};
use crate::process_supervision::{run_captured, CaptureError};
use crate::task_mode::{is_inherit_mode, requested_task_mode, INHERIT_MODE};

const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION_PROBE_OUTPUT_CAP: usize = 64 * 1024;

/// Runtime knobs read fresh per call (config reloads apply immediately).
#[derive(Debug, Clone)]
pub struct ExternalRuntimeCfg {
    pub timeout: Duration,
    pub max_concurrent: u32,
}

#[derive(Debug)]
pub struct ZodeTaskTool {
    inner: Arc<dyn Tool>,
    registry: Arc<ExternalAgentRegistry>,
    grants: Arc<GrantStore>,
    gate: Arc<dyn ApprovalGate>,
    observer: Arc<dyn TaskObserver>,
    file_cache: Arc<FileStateCache>,
    cfg: ExternalRuntimeCfg,
    description: String,
}

impl ZodeTaskTool {
    pub fn new(
        inner: Arc<dyn Tool>,
        registry: Arc<ExternalAgentRegistry>,
        grants: Arc<GrantStore>,
        gate: Arc<dyn ApprovalGate>,
        observer: Arc<dyn TaskObserver>,
        file_cache: Arc<FileStateCache>,
        cfg: ExternalRuntimeCfg,
    ) -> Self {
        let description = format!(
            "{} External agent_types run the named CLI in-place under a one-time trust approval.",
            inner.description()
        );
        Self {
            inner,
            registry,
            grants,
            gate,
            observer,
            file_cache,
            cfg,
            description,
        }
    }

    /// The structured trust view shown to approval gates. Original Task fields
    /// remain available to scoped policy matchers; `_kind` and the trusted
    /// metadata drive dedicated renderers, which show only `_prompt`'s summary.
    fn trust_view(
        def: &ExternalAgentDef,
        fp: &Fingerprint,
        task_input: &serde_json::Value,
    ) -> serde_json::Value {
        let argv = std::iter::once(def.command.display().to_string())
            .chain(def.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        let prompt = task_input
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let mut view = task_input.as_object().cloned().unwrap_or_default();
        view.insert("_kind".to_string(), serde_json::json!("external-agent"));
        view.insert("_agent".to_string(), serde_json::json!(def.name));
        view.insert("_command".to_string(), serde_json::json!(argv));
        view.insert(
            "_cwd".to_string(),
            serde_json::json!(fp.cwd.display().to_string()),
        );
        view.insert("_env".to_string(), serde_json::json!(fp.env_names));
        view.insert(
            "_sandbox".to_string(),
            serde_json::json!(fp.effective_sandbox),
        );
        view.insert(
            "_version".to_string(),
            serde_json::json!(fp
                .version_output
                .clone()
                .unwrap_or_else(|| "unverified".to_string())),
        );
        view.insert(
            "_hash".to_string(),
            serde_json::json!(&fp.content_hash[..16.min(fp.content_hash.len())]),
        );
        view.insert(
            "_prompt".to_string(),
            serde_json::json!(prompt.chars().take(200).collect::<String>()),
        );
        serde_json::Value::Object(view)
    }

    async fn call_external(
        &self,
        ctx: &ToolUseContext,
        def: &ExternalAgentDef,
        task_input: &serde_json::Value,
        prompt: String,
        description: Option<String>,
    ) -> Result<serde_json::Value, AgentError> {
        // Depth guard, symmetric with the upstream TaskTool.
        if ctx.task_depth >= DEFAULT_MAX_DEPTH {
            return Err(AgentError::other(format!(
                "Task recursion depth limit ({DEFAULT_MAX_DEPTH}) reached"
            )));
        }
        // Process-wide concurrency cap (fail fast; the model can serialize).
        let Some(_permit) = ExternalLimiter::acquire(self.cfg.max_concurrent) else {
            return Err(AgentError::other(
                "external agent concurrency limit reached; retry later or run serially",
            ));
        };

        let fp = preapproval_fingerprint(def, &ctx.cwd)
            .map_err(|e| AgentError::other(format!("cannot fingerprint {}: {e}", def.name)))?;

        let mut check = self.grants.check(&def.name, &fp);
        let mut one_shot_approved = false;
        if check == GrantCheck::Missing {
            if !self.gate.interactive() {
                if def.trusted {
                    // Explicit config opt-in substitutes for interactivity.
                    self.grants.store_pending(&def.name, fp.clone());
                    check = GrantCheck::Pending;
                } else {
                    return Err(AgentError::other(format!(
                        "external agent '{}' requires an interactive trust approval; \
                         bypass mode cannot grant it (set externalAgents.agents.{}.trusted=true \
                         to opt in explicitly)",
                        def.name, def.name
                    )));
                }
            } else {
                let view = Self::trust_view(def, &fp, task_input);
                match self
                    .gate
                    .approve_scoped("Task", &view, ApprovalScope::CarryFingerprintGrant)
                    .await
                {
                    Approval::Deny => {
                        return Err(AgentError::other(format!(
                            "external agent '{}' denied by user",
                            def.name
                        )));
                    }
                    Approval::AllowOnce => one_shot_approved = true,
                    Approval::AllowAlways => {
                        self.grants.store_pending(&def.name, fp.clone());
                        check = GrantCheck::Pending;
                    }
                }
            }
        }

        // Pre-spawn re-hash: close the approval→spawn swap window.
        let fp2 = preapproval_fingerprint(def, &ctx.cwd)
            .map_err(|e| AgentError::other(format!("cannot re-hash {}: {e}", def.name)))?;
        if fp2.content_hash != fp.content_hash {
            self.grants.revoke(&def.name);
            return Err(AgentError::other(format!(
                "executable for '{}' changed between approval and spawn; grant revoked — retry to re-approve",
                def.name
            )));
        }

        // First run after an approval: probe --version (the binary is now
        // trusted enough to execute) and finalize the fingerprint.
        if check != GrantCheck::Granted {
            let version = match probe_version(def, &ctx.abort).await {
                Ok(version) => version,
                Err(AgentError::Aborted(reason)) => {
                    self.grants.revoke(&def.name);
                    return Err(AgentError::Aborted(reason));
                }
                Err(error) => {
                    self.grants.revoke(&def.name);
                    return Err(AgentError::other(format!(
                        "version probe for '{}' failed: {error}",
                        def.name
                    )));
                }
            };
            if let Some(req) = &def.capability.version_requirement {
                if !version_satisfies(&version, req) {
                    self.grants.revoke(&def.name);
                    return Err(AgentError::other(format!(
                        "'{}' version {version:?} does not satisfy requirement {req:?}; grant revoked",
                        def.name
                    )));
                }
            }
            if !one_shot_approved {
                self.grants.promote(&def.name, version);
            }
        }

        let mut observation = TaskFinishGuard::start(
            self.observer.clone(),
            &def.name,
            description.as_deref(),
            ctx.task_depth,
        );
        let obs_id = observation.id();
        let observer = self.observer.clone();
        let spec = RunSpec {
            def: def.clone(),
            prompt,
            cwd: ctx.cwd.clone(),
            timeout: self.cfg.timeout,
            extra_args: vec![],
            file_cache: Some(self.file_cache.clone()),
        };
        let outcome = run_external(
            spec,
            |ev| {
                let event = match ev {
                    ExtEvent::Text(delta) => Event::TextDelta { delta },
                    ExtEvent::ToolUse { name, summary } => Event::ToolUse {
                        id: String::new(),
                        name,
                        input: serde_json::json!({ "summary": summary }),
                    },
                    ExtEvent::Log(message) => Event::Notice {
                        code: "external-log".to_string(),
                        message,
                    },
                };
                observer.on_event(obs_id, &event);
            },
            ctx.abort.child(),
        )
        .await;

        match outcome {
            Ok(out) => {
                observation.finish(&out.result.text, None);
                Ok(serde_json::json!({
                    "output": out.result.text,
                    "agent_type": def.name,
                    "exit_code": out.exit_code,
                    "duration_ms": out.duration_ms,
                    "changed_files": out.changed_files,
                    "session_id": out.result.session_id,
                    "__external_agent__": {
                        "profile": def.name,
                        "model": out.result.model,
                        "usage_input_tokens": out.result.usage_in,
                        "usage_output_tokens": out.result.usage_out,
                    }
                }))
            }
            Err(e) => {
                observation.finish("", Some(&e));
                Err(AgentError::other(e))
            }
        }
    }
}

/// Run `<command> --version` under the same env hygiene as a real run and
/// return the first output line. The shared supervisor bounds output, observes
/// root cancellation, and owns the entire process group through termination.
async fn probe_version(
    def: &ExternalAgentDef,
    abort: &agent::abort::AbortController,
) -> Result<String, AgentError> {
    let mut cmd = tokio::process::Command::new(&def.command);
    cmd.arg("--version");
    cmd.env_clear();
    for name in ["PATH", "HOME", "TERM"] {
        if let Ok(v) = std::env::var(name) {
            cmd.env(name, v);
        }
    }
    let out = match run_captured(cmd, abort, VERSION_PROBE_TIMEOUT, VERSION_PROBE_OUTPUT_CAP).await
    {
        Ok(output) => output,
        Err(CaptureError::Aborted(reason)) => return Err(AgentError::Aborted(reason)),
        Err(error) => return Err(AgentError::other(error.to_string())),
    };
    if !out.status.success() {
        return Err(AgentError::other(format!(
            "exit {}",
            out.status.code().unwrap_or(-1)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string())
}

/// Loose semver floor check: extract the first dotted-number run from
/// `version_output` and compare component-wise against `req` (leading
/// ">=" optional). Unparseable output fails closed.
fn version_satisfies(version_output: &str, req: &str) -> bool {
    fn parse(s: &str) -> Option<Vec<u64>> {
        let start = s.find(|c: char| c.is_ascii_digit())?;
        let run: String = s[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<u64> = run.split('.').filter_map(|p| p.parse().ok()).collect();
        (!parts.is_empty()).then_some(parts)
    }
    let Some(actual) = parse(version_output) else {
        return false;
    };
    let Some(floor) = parse(req.trim_start_matches(">=").trim()) else {
        return false;
    };
    for i in 0..floor.len().max(actual.len()) {
        let a = actual.get(i).copied().unwrap_or(0);
        let f = floor.get(i).copied().unwrap_or(0);
        if a != f {
            return a > f;
        }
    }
    true
}

#[async_trait]
impl Tool for ZodeTaskTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn input_schema(&self) -> serde_json::Value {
        self.inner.input_schema()
    }
    fn safety_class(&self) -> SafetyClass {
        self.inner.safety_class()
    }

    async fn call(
        &self,
        ctx: &ToolUseContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        let agent_type = input
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(def) = self.registry.get(agent_type) else {
            // Internal route: verbatim pass-through (Phase A red line).
            return self.inner.call(ctx, input).await;
        };
        // External CLIs run in-place and do not inherit Zode's immutable
        // child-loop tool registry, so Zode cannot enforce modes such as
        // `plan` for them. Reject every capability-changing mode before
        // fingerprinting, approval, version probing, or spawning the command.
        // Internal agent types remain routed through the inner Task mode
        // router above.
        let requested_mode = requested_task_mode(&input)?;
        if !is_inherit_mode(requested_mode) {
            return Err(AgentError::other(format!(
                "external agent_type '{agent_type}' does not support Task mode \
                 '{requested_mode}'; use 'inherit' or 'default'"
            )));
        }
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::other("Task requires a 'prompt' string"))?
            .to_string();
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let def = def.clone();
        let mut output = self
            .call_external(ctx, &def, &input, prompt, description)
            .await?;
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "requested_mode".to_string(),
                serde_json::json!(requested_mode),
            );
            object.insert("mode".to_string(), serde_json::json!(INHERIT_MODE));
        }
        Ok(output)
    }
}

#[cfg(test)]
#[path = "task-tool-policy-tests.rs"]
mod policy_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExternalAgentsConfig;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct StubInner {
        calls: Mutex<Vec<serde_json::Value>>,
    }
    #[async_trait]
    impl Tool for StubInner {
        fn name(&self) -> &str {
            "Task"
        }
        fn description(&self) -> &str {
            "inner task."
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _c: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            self.calls.lock().unwrap().push(input.clone());
            Ok(json!({"output":"internal-ok"}))
        }
    }

    #[derive(Debug)]
    struct FixedGate(Approval, bool);
    #[async_trait]
    impl ApprovalGate for FixedGate {
        async fn approve(&self, _t: &str, _i: &serde_json::Value) -> Approval {
            self.0
        }
        fn interactive(&self) -> bool {
            self.1
        }
    }

    #[derive(Debug, Default)]
    struct NullObserver;
    impl TaskObserver for NullObserver {
        fn on_start(&self, _a: &str, _d: Option<&str>, _depth: usize) -> u64 {
            1
        }
        fn on_event(&self, _id: u64, _e: &Event) {}
        fn on_finish(&self, _id: u64, _r: &str, _e: Option<&str>) {}
    }

    fn fixture(script: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/extagent")
            .join(script)
    }

    fn registry_with(name: &str, script: &str, trusted: bool) -> Arc<ExternalAgentRegistry> {
        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            name.to_string(),
            serde_json::from_value(json!({
                "command": fixture(script).display().to_string(),
                "args": [],
                "promptTransport": "stdin",
                "output": "jsonl-claude",
                "trusted": trusted,
            }))
            .unwrap(),
        );
        Arc::new(crate::external_agents::discover(&cfg, &[]))
    }

    fn tool_with(
        registry: Arc<ExternalAgentRegistry>,
        gate: FixedGate,
        grants: Arc<GrantStore>,
    ) -> ZodeTaskTool {
        tool_with_observer(registry, gate, grants, Arc::new(NullObserver))
    }

    fn tool_with_observer(
        registry: Arc<ExternalAgentRegistry>,
        gate: FixedGate,
        grants: Arc<GrantStore>,
        observer: Arc<dyn TaskObserver>,
    ) -> ZodeTaskTool {
        ZodeTaskTool::new(
            Arc::new(StubInner::default()),
            registry,
            grants,
            Arc::new(gate),
            observer,
            Arc::new(FileStateCache::new(
                std::num::NonZeroUsize::new(8).unwrap(),
                1 << 20,
            )),
            ExternalRuntimeCfg {
                timeout: Duration::from_secs(30),
                max_concurrent: 4,
            },
        )
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn external_non_inherit_modes_are_rejected_before_command_start() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("command-started");
        let script = dir.path().join("external-agent.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf started > '{}'\nprintf '%s\\n' \
                 '{{\"type\":\"result\",\"result\":\"unexpected\"}}'\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            "mode-probe".to_string(),
            serde_json::from_value(json!({
                "command": script.display().to_string(),
                "args": [],
                "promptTransport": "stdin",
                "output": "jsonl-claude",
                "trusted": true,
            }))
            .unwrap(),
        );
        let registry = Arc::new(crate::external_agents::discover(&cfg, &[]));
        assert!(registry.get("mode-probe").is_some());
        let tool = tool_with(
            registry,
            FixedGate(Approval::AllowOnce, false),
            Default::default(),
        );

        for mode in ["plan", "read-only"] {
            let error = tool
                .call(
                    &ctx(),
                    json!({
                        "agent_type": "mode-probe",
                        "prompt": "do not run",
                        "mode": mode
                    }),
                )
                .await
                .unwrap_err();

            let message = error.to_string();
            assert!(message.contains("mode-probe"), "{message}");
            assert!(message.contains(mode), "{message}");
            assert!(message.contains("inherit"), "{message}");
        }
        assert!(
            !marker.exists(),
            "external command (including its version probe) must not start"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    #[serial_test::serial]
    async fn external_missing_grant_with_noninteractive_gate_fails_closed() {
        let reg = registry_with("fake-ext", "fake-claude.sh", false);
        let tool = tool_with(
            reg,
            FixedGate(Approval::AllowOnce, false),
            Default::default(),
        );
        let err = tool
            .call(&ctx(), json!({"agent_type":"fake-ext","prompt":"go"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("trusted=true"), "{err}");
    }

    #[tokio::test]
    #[cfg(unix)]
    #[serial_test::serial]
    async fn external_trusted_profile_runs_under_bypass() {
        let reg = registry_with("fake-ext", "fake-claude.sh", true);
        let grants = Arc::new(GrantStore::default());
        let observed = crate::subagents::SubAgentRegistry::new();
        let tool = tool_with_observer(
            reg,
            FixedGate(Approval::AllowOnce, false),
            grants,
            observed.observer(),
        );
        let mut context = ctx();
        context.task_depth = 1;
        let out = tool
            .call(
                &context,
                json!({
                    "agent_type":"fake-ext",
                    "prompt":"go",
                    "description":"nested external",
                    "mode":"default"
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["__external_agent__"]["profile"], "fake-ext");
        assert_eq!(out["session_id"], "sess-0001");
        assert_eq!(out["requested_mode"], "default");
        assert_eq!(out["mode"], "inherit");
        let agents = observed.snapshot();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].depth, 1);
    }

    #[tokio::test]
    #[cfg(unix)]
    #[serial_test::serial]
    async fn external_deny_is_an_error_and_gate_allow_session_grants() {
        let reg = registry_with("fake-ext", "fake-claude.sh", false);
        let grants = Arc::new(GrantStore::default());
        let tool = tool_with(reg.clone(), FixedGate(Approval::Deny, true), grants.clone());
        assert!(tool
            .call(&ctx(), json!({"agent_type":"fake-ext","prompt":"go"}))
            .await
            .is_err());

        let tool = tool_with(reg, FixedGate(Approval::AllowAlways, true), grants.clone());
        let out = tool
            .call(&ctx(), json!({"agent_type":"fake-ext","prompt":"go"}))
            .await
            .unwrap();
        assert_eq!(out["output"], "完成：已更新 src/a.rs");
        // grant promoted with a version -> next check is Granted
        let def = registry_with("fake-ext", "fake-claude.sh", false)
            .get("fake-ext")
            .unwrap()
            .clone();
        let fp = preapproval_fingerprint(&def, &std::env::temp_dir()).unwrap();
        assert_eq!(grants.check("fake-ext", &fp), GrantCheck::Granted);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn external_depth_guard_matches_upstream() {
        let reg = registry_with("fake-ext", "fake-claude.sh", true);
        let tool = tool_with(
            reg,
            FixedGate(Approval::AllowOnce, true),
            Default::default(),
        );
        let mut c = ctx();
        c.task_depth = DEFAULT_MAX_DEPTH;
        let err = tool
            .call(&c, json!({"agent_type":"fake-ext","prompt":"go"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("depth"));
    }

    #[test]
    fn version_floor_comparison() {
        assert!(version_satisfies("fake 1.2.3", ">=1.2"));
        assert!(version_satisfies("2.0", ">=1.9.9"));
        assert!(!version_satisfies("1.1", ">=1.2"));
        assert!(!version_satisfies("no digits here", ">=1.0"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn version_probe_honors_root_abort_before_spawn() {
        let registry = registry_with("fake-ext", "fake-claude.sh", true);
        let def = registry.get("fake-ext").unwrap();
        let abort = agent::abort::AbortController::new();
        abort.abort_with_reason("watchdog stop");

        let error = probe_version(def, &abort).await.unwrap_err();

        assert!(matches!(error, AgentError::Aborted(reason) if reason == "watchdog stop"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_version_probe_kills_descendants() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("version-probe.sh");
        let pid_file = dir.path().join("probe-child.pid");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf %s \"$child\" > '{}'\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            "probe".to_string(),
            serde_json::from_value(json!({
                "command": script.display().to_string(),
                "args": [],
                "promptTransport": "stdin",
                "output": "jsonl-claude",
                "trusted": true,
            }))
            .unwrap(),
        );
        let registry = crate::external_agents::discover(&cfg, &[]);
        let def = registry.get("probe").unwrap().clone();
        let abort = agent::abort::AbortController::new();
        let probe_abort = abort.clone();
        let probe = tokio::spawn(async move { probe_version(&def, &probe_abort).await });
        let pid = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(raw) = tokio::fs::read_to_string(&pid_file).await {
                    if let Ok(pid) = raw.trim().parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("version probe did not write its child pid");

        abort.abort_with_reason("watchdog stop");
        let error = probe.await.unwrap().unwrap_err();
        assert!(matches!(error, AgentError::Aborted(reason) if reason == "watchdog stop"));

        for _ in 0..100 {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .is_ok_and(|output| output.status.success());
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("version-probe descendant {pid} survived abort");
    }
}
