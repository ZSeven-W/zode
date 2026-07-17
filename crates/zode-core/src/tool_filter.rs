//! Runtime tool selection for headless and protocol clients.
//!
//! An empty allowlist means "all assembled tools". Deny patterns are applied
//! after allow patterns and therefore always win. Patterns use anchored shell
//! wildcards: `*` matches any sequence and `?` one Unicode scalar value.

use std::collections::BTreeSet;

use agent::tool::ToolRegistry;

use crate::CoreError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFilter {
    allow: Vec<String>,
    deny: Vec<String>,
}

impl ToolFilter {
    pub fn new(allow: Vec<String>, deny: Vec<String>) -> Result<Self, CoreError> {
        if let Some(empty) = allow
            .iter()
            .chain(&deny)
            .find(|pattern| pattern.trim().is_empty())
        {
            return Err(CoreError::Other(format!(
                "tool filter contains an empty pattern: {empty:?}"
            )));
        }
        Ok(Self {
            allow: dedup(allow),
            deny: dedup(deny),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }

    pub fn allows(&self, name: &str) -> bool {
        let allowed =
            self.allow.is_empty() || self.allow.iter().any(|pattern| glob_matches(pattern, name));
        allowed && !self.deny.iter().any(|pattern| glob_matches(pattern, name))
    }

    pub fn apply(&self, registry: ToolRegistry) -> Result<ToolRegistry, CoreError> {
        self.apply_with_virtual(registry, &[])
    }

    /// Filter `registry`, additionally validating patterns against
    /// `virtual_names` — tools the engine registers after the filter runs
    /// (e.g. ToolSearch). The caller checks [`Self::allows`] before
    /// registering each virtual tool.
    pub fn apply_with_virtual(
        &self,
        registry: ToolRegistry,
        virtual_names: &[&str],
    ) -> Result<ToolRegistry, CoreError> {
        self.validate_matches(&registry, virtual_names)?;
        let mut filtered = ToolRegistry::new();
        for tool in registry.list() {
            if self.allows(tool.name()) {
                filtered.register(tool);
            }
        }
        Ok(filtered)
    }

    fn validate_matches(
        &self,
        registry: &ToolRegistry,
        virtual_names: &[&str],
    ) -> Result<(), CoreError> {
        let names: Vec<&str> = registry
            .names()
            .chain(virtual_names.iter().copied())
            .collect();
        for pattern in self.allow.iter().chain(&self.deny) {
            if !names.iter().any(|name| glob_matches(pattern, name)) {
                return Err(CoreError::Other(format!(
                    "tool pattern '{pattern}' matched no available tools"
                )));
            }
        }
        Ok(())
    }
}

fn dedup(patterns: Vec<String>) -> Vec<String> {
    patterns
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Anchored `*`/`?` glob via the agent runtime's matcher — the same
/// semantics permission rules use, so a pattern string means one thing
/// across `--tools`, `--disallowed-tools`, and permission rules.
fn glob_matches(pattern: &str, text: &str) -> bool {
    agent::permission::StringPattern::glob(pattern).matches(text)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agent::testing::FakeTool;

    use super::*;

    fn registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        for name in ["FileRead", "FileEdit", "Bash", "mcp__github__read"] {
            registry.register(Arc::new(FakeTool::new(name, serde_json::json!({}))));
        }
        registry
    }

    #[test]
    fn deny_wins_over_allow_and_globs_are_supported() {
        let filter =
            ToolFilter::new(vec!["File*".into(), "Bash".into()], vec!["*Edit".into()]).unwrap();
        let filtered = filter.apply(registry()).unwrap();
        let names: BTreeSet<_> = filtered.names().collect();
        assert_eq!(names, BTreeSet::from(["Bash", "FileRead"]));
    }

    #[test]
    fn empty_allowlist_means_all_except_denied() {
        let filter = ToolFilter::new(vec![], vec!["Bash".into()]).unwrap();
        let filtered = filter.apply(registry()).unwrap();
        assert!(filtered.get("FileRead").is_some());
        assert!(filtered.get("Bash").is_none());
    }

    #[test]
    fn unmatched_pattern_is_an_error_not_a_silent_typo() {
        let filter = ToolFilter::new(vec!["NoSuchTool".into()], vec![]).unwrap();
        let error = filter.apply(registry()).unwrap_err().to_string();
        assert!(error.contains("matched no available tools"), "{error}");
    }

    #[test]
    fn virtual_names_validate_patterns_for_late_registered_tools() {
        // ToolSearch is registered after the filter runs; a pattern naming it
        // must validate against the virtual set instead of erroring, and the
        // filter must report it as denied so the engine skips registration.
        let filter = ToolFilter::new(vec![], vec!["ToolSearch".into()]).unwrap();
        let filtered = filter
            .apply_with_virtual(registry(), &["ToolSearch"])
            .unwrap();
        assert!(filtered.get("FileRead").is_some());
        assert!(!filter.allows("ToolSearch"));
        // An allowlist that doesn't name ToolSearch excludes it too.
        let allowlist = ToolFilter::new(vec!["FileRead".into()], vec![]).unwrap();
        assert!(!allowlist.allows("ToolSearch"));
    }

    #[test]
    fn wildcard_matches_unicode_scalars() {
        assert!(glob_matches("工具?", "工具一"));
        assert!(glob_matches("mcp__*__read", "mcp__github__read"));
        assert!(!glob_matches("File?", "FileRead"));
    }
}
