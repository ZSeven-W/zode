//! External agent CLIs (claude / codex / opencode / custom) exposed as Task
//! `agent_type`s. Design: docs/superpowers/specs/2026-07-16-agent-team-design.md
//! (v2.3) §3 — discovery is stat-only, execution is trust-delegated (ADR-2),
//! and the Task route is self-gated (ADR-4).

pub mod capability;
pub mod fingerprint;
pub mod limiter;
pub mod parser;
pub mod profiles;

use std::path::{Path, PathBuf};

use crate::config::ExternalAgentsConfig;

pub use capability::{EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport};
pub use fingerprint::{hash_file, preapproval_fingerprint, Fingerprint, GrantCheck, GrantStore};
pub use profiles::{builtin_profiles, def_from_entry, ExternalAgentDef};

/// The set of external agents available as Task `agent_type`s this session.
/// Built once at engine assembly; a disabled entry keeps its reason for the
/// startup log but is invisible to the model.
#[derive(Debug, Default)]
pub struct ExternalAgentRegistry {
    agents: Vec<ExternalAgentDef>,
    disabled: Vec<(String, String)>,
}

impl ExternalAgentRegistry {
    pub fn get(&self, name: &str) -> Option<&ExternalAgentDef> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// `(name, one-line summary)` pairs for the system-prompt agent list.
    pub fn agent_types(&self) -> Vec<(String, String)> {
        self.agents
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    format!(
                        "external CLI agent ({}); runs in-place under a one-time trust approval",
                        a.command.display()
                    ),
                )
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Entries disabled at discovery, with reasons (startup diagnostics).
    pub fn disabled(&self) -> &[(String, String)] {
        &self.disabled
    }
}

/// Resolve `binary` on a sanitized PATH (relative entries and "." dropped) to
/// a canonical absolute path. Stat-only — never executes the candidate.
fn resolve_binary(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.is_relative() || dir == Path::new(".") {
            continue;
        }
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return candidate.canonicalize().ok();
        }
        #[cfg(windows)]
        {
            if let Some(exts) = std::env::var_os("PATHEXT") {
                for ext in std::env::split_paths(&exts) {
                    let ext = ext.to_string_lossy();
                    let ext = ext.trim_start_matches('.');
                    let cand = dir.join(format!("{binary}.{ext}"));
                    if cand.is_file() {
                        return cand.canonicalize().ok();
                    }
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(p: &Path) -> bool {
    p.is_file()
}

/// Build the registry from built-in profiles found on PATH plus custom config
/// entries. `builtin_conflicts` carries names already taken by built-in
/// agent types / user AgentDefs — a colliding external entry is disabled
/// loudly rather than silently shadowed.
pub fn discover(cfg: &ExternalAgentsConfig, builtin_conflicts: &[String]) -> ExternalAgentRegistry {
    let mut reg = ExternalAgentRegistry::default();
    if !cfg.enabled() {
        return reg;
    }

    for (name, binary, capability, args, auth_env) in builtin_profiles() {
        let entry = cfg.agents.get(name);
        if entry.and_then(|e| e.enabled) == Some(false) {
            reg.disabled
                .push((name.to_string(), "disabled by config".to_string()));
            continue;
        }
        if builtin_conflicts.iter().any(|c| c == name) {
            reg.disabled.push((
                name.to_string(),
                "name collides with a built-in agent type or AgentDef".to_string(),
            ));
            continue;
        }
        let command = match entry.and_then(|e| e.command.as_deref()) {
            Some(explicit) => match Path::new(explicit).canonicalize() {
                Ok(p) if is_executable_file(&p) => p,
                _ => {
                    reg.disabled.push((
                        name.to_string(),
                        format!("configured command not found: {explicit}"),
                    ));
                    continue;
                }
            },
            None => match resolve_binary(binary) {
                Some(p) => p,
                None => continue, // not installed — silently absent
            },
        };
        let mut args = args;
        if let Some(extra) = entry.and_then(|e| e.extra_args.clone()) {
            args.extend(extra);
        }
        reg.agents.push(ExternalAgentDef {
            name: name.to_string(),
            command,
            args,
            capability,
            auth_env,
            env_allow: entry.and_then(|e| e.env_allow.clone()).unwrap_or_default(),
            trusted: entry.and_then(|e| e.trusted).unwrap_or(false),
        });
    }

    let builtin_names: Vec<&str> = builtin_profiles().iter().map(|p| p.0).collect();
    for (name, entry) in &cfg.agents {
        if builtin_names.contains(&name.as_str()) {
            continue; // handled above as overrides
        }
        if entry.enabled == Some(false) {
            reg.disabled
                .push((name.clone(), "disabled by config".to_string()));
            continue;
        }
        if builtin_conflicts.iter().any(|c| c == name) {
            reg.disabled.push((
                name.clone(),
                "name collides with a built-in agent type or AgentDef".to_string(),
            ));
            continue;
        }
        match def_from_entry(name, entry) {
            Ok(mut def) => match def.command.canonicalize() {
                Ok(p) if is_executable_file(&p) => {
                    def.command = p;
                    reg.agents.push(def);
                }
                _ => reg.disabled.push((
                    name.clone(),
                    format!("command not found: {}", def.command.display()),
                )),
            },
            Err(reason) => reg.disabled.push((name.clone(), reason)),
        }
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn discovery_is_stat_only_and_skips_missing() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, b"bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());
        let reg = discover(&Default::default(), &[]);
        if let Some(p) = old_path {
            std::env::set_var("PATH", p);
        }
        assert!(reg.get("claude-code").is_some());
        assert!(reg.get("codex").is_none());
        assert!(!reg.agent_types().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn name_conflicts_disable_entry_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, b"bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());
        let reg = discover(&Default::default(), &["claude-code".to_string()]);
        if let Some(p) = old_path {
            std::env::set_var("PATH", p);
        }
        assert!(reg.get("claude-code").is_none());
        assert!(reg
            .disabled()
            .iter()
            .any(|(n, r)| n == "claude-code" && r.contains("collides")));
    }
}
