use super::*;
use agent::permission::{PermissionBehavior, PermissionRule, RuleSource, StringPattern};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug)]
struct NullTask;

#[async_trait]
impl Tool for NullTask {
    fn name(&self) -> &str {
        "Task"
    }

    fn description(&self) -> &str {
        "Test Task tool."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn call(
        &self,
        _ctx: &ToolUseContext,
        _input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        Ok(json!({"output": "internal"}))
    }
}

#[derive(Debug)]
struct NullObserver;

impl TaskObserver for NullObserver {
    fn on_start(&self, _agent_type: &str, _description: Option<&str>, _depth: usize) -> u64 {
        1
    }

    fn on_event(&self, _id: u64, _event: &Event) {}

    fn on_finish(&self, _id: u64, _result: &str, _error: Option<&str>) {}
}

#[derive(Debug)]
struct CountingDenyGate(Arc<AtomicUsize>);

#[async_trait]
impl ApprovalGate for CountingDenyGate {
    fn interactive(&self) -> bool {
        true
    }

    async fn approve(&self, _tool: &str, _input: &serde_json::Value) -> Approval {
        self.0.fetch_add(1, Ordering::SeqCst);
        Approval::Deny
    }
}

#[derive(Debug, Default)]
struct RecordingTask {
    calls: Mutex<Vec<serde_json::Value>>,
}

#[async_trait]
impl Tool for RecordingTask {
    fn name(&self) -> &str {
        "Task"
    }

    fn description(&self) -> &str {
        "Recording Task tool."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn call(
        &self,
        _ctx: &ToolUseContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        self.calls.lock().unwrap().push(input);
        Ok(json!({"output": "internal-ok"}))
    }
}

#[tokio::test]
async fn internal_agent_type_passes_through_untouched() {
    let inner = Arc::new(RecordingTask::default());
    let tool = ZodeTaskTool::new(
        inner.clone(),
        Arc::new(ExternalAgentRegistry::default()),
        Arc::new(GrantStore::default()),
        Arc::new(CountingDenyGate(Arc::new(AtomicUsize::new(0)))),
        Arc::new(NullObserver),
        Arc::new(FileStateCache::new(
            std::num::NonZeroUsize::new(8).unwrap(),
            1 << 20,
        )),
        ExternalRuntimeCfg {
            timeout: Duration::from_secs(1),
            max_concurrent: 1,
        },
    );
    let input = json!({"agent_type":"general","prompt":"hi","mode":"plan"});

    let output = tool
        .call(&ToolUseContext::new(std::env::temp_dir()), input.clone())
        .await
        .unwrap();

    assert_eq!(output["output"], "internal-ok");
    assert_eq!(inner.calls.lock().unwrap()[0], input);
}

#[tokio::test]
#[cfg(unix)]
async fn external_trust_gate_matches_scoped_rules_against_original_task_input() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extagent/fake-claude.sh");
    let mut cfg = crate::config::ExternalAgentsConfig::default();
    cfg.agents.insert(
        "fake-ext".to_string(),
        serde_json::from_value(json!({
            "command": fixture.display().to_string(),
            "args": [],
            "promptTransport": "stdin",
            "output": "jsonl-claude",
        }))
        .unwrap(),
    );
    let registry = Arc::new(crate::external_agents::discover(&cfg, &[]));
    let prompts = Arc::new(AtomicUsize::new(0));
    let rules = vec![
        PermissionRule::whole_tool(RuleSource::User, PermissionBehavior::Allow, "Task"),
        PermissionRule::with_input_match(
            RuleSource::Policy,
            PermissionBehavior::Ask,
            "Task",
            "/agent_type",
            StringPattern::glob("fake-ext"),
        ),
    ];
    let gate = Arc::new(crate::permission_rules::RuleApprovalGate::new(
        Arc::new(CountingDenyGate(prompts.clone())),
        rules,
    ));
    let tool = ZodeTaskTool::new(
        Arc::new(NullTask),
        registry,
        Arc::new(GrantStore::default()),
        gate,
        Arc::new(NullObserver),
        Arc::new(FileStateCache::new(
            std::num::NonZeroUsize::new(8).unwrap(),
            1 << 20,
        )),
        ExternalRuntimeCfg {
            timeout: Duration::from_secs(30),
            max_concurrent: 1,
        },
    );

    let error = tool
        .call(
            &ToolUseContext::new(std::env::temp_dir()),
            json!({
                "agent_type": "fake-ext",
                "prompt": "policy-visible prompt",
                "mode": "default"
            }),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("denied"), "{error}");
    assert_eq!(
        prompts.load(Ordering::SeqCst),
        1,
        "scoped ask must win over the whole-tool allow"
    );
}
