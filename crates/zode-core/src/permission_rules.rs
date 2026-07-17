use std::path::Path;
use std::sync::Arc;

use agent::permission::{PermissionBehavior, PermissionMatcher, PermissionRule, RuleSource};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::approval::{Approval, ApprovalGate};
use crate::config::PermissionsConfig;
use crate::CoreError;

/// User-facing rule format. Source/provenance is assigned by the loader, so a
/// rules file cannot claim the higher precedence of policy-owned rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionRuleSpec {
    pub behavior: PermissionBehavior,
    #[serde(alias = "tool_name")]
    pub tool: String,
    #[serde(default)]
    pub matcher: PermissionMatcher,
}

impl PermissionRuleSpec {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.tool.trim().is_empty() {
            return Err(CoreError::Other("permission rule tool is empty".into()));
        }
        validate_matcher(&self.matcher)
    }

    pub fn to_rule(&self, source: RuleSource) -> Result<PermissionRule, CoreError> {
        self.validate()?;
        Ok(PermissionRule::with_matcher(
            source,
            self.behavior,
            self.tool.clone(),
            self.matcher.clone(),
        ))
    }
}

fn validate_matcher(matcher: &PermissionMatcher) -> Result<(), CoreError> {
    match matcher {
        PermissionMatcher::Field { pointer, .. } if !pointer.starts_with('/') => {
            Err(CoreError::Other(format!(
                "permission matcher pointer must start with '/': {pointer}"
            )))
        }
        PermissionMatcher::AnyOf { matchers } | PermissionMatcher::AllOf { matchers } => {
            for matcher in matchers {
                validate_matcher(matcher)?;
            }
            Ok(())
        }
        PermissionMatcher::Not { matcher } => validate_matcher(matcher),
        _ => Ok(()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RuleDocument {
    List(Vec<PermissionRuleSpec>),
    Object { rules: Vec<PermissionRuleSpec> },
}

pub fn load_rule_file(path: &Path, source: RuleSource) -> Result<Vec<PermissionRule>, CoreError> {
    load_rule_specs(path)?
        .iter()
        .map(|spec| spec.to_rule(source))
        .collect()
}

pub fn load_rule_specs(path: &Path) -> Result<Vec<PermissionRuleSpec>, CoreError> {
    let bytes = std::fs::read(path)?;
    let document: RuleDocument = serde_json::from_slice(&bytes)?;
    let specs = match document {
        RuleDocument::List(rules) | RuleDocument::Object { rules } => rules,
    };
    for spec in &specs {
        spec.validate()?;
    }
    Ok(specs)
}

pub fn config_rules(config: &PermissionsConfig) -> Result<Vec<PermissionRule>, CoreError> {
    let mut rules = Vec::new();
    for tool in &config.allow {
        rules.push(PermissionRule::whole_tool(
            RuleSource::User,
            PermissionBehavior::Allow,
            tool,
        ));
    }
    for tool in &config.ask {
        rules.push(PermissionRule::whole_tool(
            RuleSource::User,
            PermissionBehavior::Ask,
            tool,
        ));
    }
    for tool in &config.deny {
        rules.push(PermissionRule::whole_tool(
            RuleSource::User,
            PermissionBehavior::Deny,
            tool,
        ));
    }
    for spec in &config.rules {
        rules.push(spec.to_rule(RuleSource::User)?);
    }
    Ok(rules)
}

/// Tools that have `ask` rules but no whole-tool (`Always` matcher) `ask`
/// rule. These are force-gated so their scoped rules can intercept matching
/// calls, but an unmatched call must keep the tool's base disposition.
pub fn scoped_ask_tools(rules: &[PermissionRule]) -> std::collections::BTreeSet<String> {
    let whole: std::collections::BTreeSet<&str> = rules
        .iter()
        .filter(|rule| {
            rule.behavior == PermissionBehavior::Ask
                && matches!(rule.matcher, PermissionMatcher::Always)
        })
        .map(|rule| rule.tool_name.as_str())
        .collect();
    rules
        .iter()
        .filter(|rule| {
            rule.behavior == PermissionBehavior::Ask && !whole.contains(rule.tool_name.as_str())
        })
        .map(|rule| rule.tool_name.clone())
        .collect()
}

/// Per-call structured policy adapter. Deny wins, then explicit ask, then
/// allow. Unmatched calls fall through to the surface's normal approval gate,
/// except for `pass_unmatched` tools (gated only by scoped ask rules and
/// otherwise ungated), which auto-allow when no rule matches — a scoped ask
/// on a read-only tool must not blanket-prompt (or blanket-deny under
/// `dont-ask`) every call.
#[derive(Debug)]
pub struct RuleApprovalGate {
    inner: Arc<dyn ApprovalGate>,
    rules: Vec<PermissionRule>,
    pass_unmatched: std::sync::OnceLock<std::collections::BTreeSet<String>>,
}

impl RuleApprovalGate {
    pub fn new(inner: Arc<dyn ApprovalGate>, rules: Vec<PermissionRule>) -> Self {
        Self {
            inner,
            rules,
            pass_unmatched: std::sync::OnceLock::new(),
        }
    }

    pub fn with_pass_unmatched(self, tools: std::collections::BTreeSet<String>) -> Self {
        let _ = self.pass_unmatched.set(tools);
        self
    }

    /// Late-bind the pass-unmatched set (engine assembly learns which tools
    /// are read-only/allow-listed only after the registry is built). Only the
    /// first call takes effect.
    pub fn set_pass_unmatched(&self, tools: std::collections::BTreeSet<String>) {
        let _ = self.pass_unmatched.set(tools);
    }

    fn decision(&self, tool: &str, input: &serde_json::Value) -> Option<PermissionBehavior> {
        [
            PermissionBehavior::Deny,
            PermissionBehavior::Ask,
            PermissionBehavior::Allow,
        ]
        .into_iter()
        .find(|behavior| {
            self.rules
                .iter()
                .any(|rule| rule.behavior == *behavior && rule.applies_to(tool, input))
        })
    }
}

#[async_trait]
impl ApprovalGate for RuleApprovalGate {
    fn interactive(&self) -> bool {
        self.inner.interactive()
    }

    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        match self.decision(tool, input) {
            Some(PermissionBehavior::Deny) => Approval::Deny,
            Some(PermissionBehavior::Allow) => Approval::AllowOnce,
            None if self
                .pass_unmatched
                .get()
                .is_some_and(|tools| tools.contains(tool)) =>
            {
                Approval::AllowOnce
            }
            Some(PermissionBehavior::Ask) | None => self.inner.approve(tool, input).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use agent::permission::StringPattern;

    use super::*;

    #[derive(Debug)]
    struct CountingGate(std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl ApprovalGate for CountingGate {
        async fn approve(&self, _tool: &str, _input: &serde_json::Value) -> Approval {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Approval::AllowOnce
        }
    }

    #[tokio::test]
    async fn deny_wins_and_scoped_allow_skips_the_prompt() {
        let base = Arc::new(CountingGate(std::sync::atomic::AtomicUsize::new(0)));
        let rules = vec![
            PermissionRule::with_input_match(
                RuleSource::CliArg,
                PermissionBehavior::Allow,
                "Bash",
                "/command",
                StringPattern::glob("git status*"),
            ),
            PermissionRule::with_input_match(
                RuleSource::CliArg,
                PermissionBehavior::Deny,
                "Bash",
                "/command",
                StringPattern::glob("*--force*"),
            ),
        ];
        let gate = RuleApprovalGate::new(base.clone(), rules);
        assert_eq!(
            gate.approve("Bash", &serde_json::json!({"command": "git status"}))
                .await,
            Approval::AllowOnce
        );
        assert_eq!(
            gate.approve(
                "Bash",
                &serde_json::json!({"command": "git status --force"})
            )
            .await,
            Approval::Deny
        );
        assert_eq!(base.0.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn scoped_ask_leaves_unmatched_inputs_ungated_for_pass_listed_tools() {
        let base = Arc::new(CountingGate(std::sync::atomic::AtomicUsize::new(0)));
        let rules = vec![PermissionRule::with_input_match(
            RuleSource::User,
            PermissionBehavior::Ask,
            "FileRead",
            "/path",
            StringPattern::glob("/etc/*"),
        )];
        let gate = RuleApprovalGate::new(base.clone(), rules)
            .with_pass_unmatched(["FileRead".to_string()].into());
        // Unmatched input: the tool would run ungated without this rule, so
        // the scoped ask must not send it to the inner (prompt/deny) gate.
        assert_eq!(
            gate.approve("FileRead", &serde_json::json!({"path": "/workspace/a.txt"}))
                .await,
            Approval::AllowOnce
        );
        assert_eq!(base.0.load(std::sync::atomic::Ordering::Relaxed), 0);
        // Matched input still defers to the inner gate.
        gate.approve("FileRead", &serde_json::json!({"path": "/etc/passwd"}))
            .await;
        assert_eq!(base.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn scoped_ask_tools_excludes_whole_tool_asks() {
        let rules = vec![
            PermissionRule::whole_tool(RuleSource::User, PermissionBehavior::Ask, "Bash"),
            PermissionRule::with_input_match(
                RuleSource::User,
                PermissionBehavior::Ask,
                "FileRead",
                "/path",
                StringPattern::glob("/etc/*"),
            ),
            // Scoped AND whole-tool ask on the same tool: whole-tool wins.
            PermissionRule::with_input_match(
                RuleSource::User,
                PermissionBehavior::Ask,
                "Bash",
                "/command",
                StringPattern::glob("rm *"),
            ),
        ];
        assert_eq!(
            scoped_ask_tools(&rules),
            std::collections::BTreeSet::from(["FileRead".to_string()])
        );
    }

    #[test]
    fn rejects_non_pointer_field_matchers() {
        let spec = PermissionRuleSpec {
            behavior: PermissionBehavior::Allow,
            tool: "Bash".into(),
            matcher: PermissionMatcher::field_glob("command", "git *"),
        };
        assert!(spec.validate().is_err());
    }
}
