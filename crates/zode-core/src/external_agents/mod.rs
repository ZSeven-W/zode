//! Manually registered external agent CLIs exposed as Task `agent_type`s.
//! Executable resolution is stat-only, execution is trust-delegated, and the
//! Task route is self-gated. Merely installing a known CLI never registers it.

pub mod capability;
pub mod fingerprint;
pub mod limiter;
pub mod parser;
pub mod profiles;
pub mod runner;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::config::{ConfigManager, ExternalAgentEntry, ExternalAgentsConfig};
use crate::CoreError;

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
fn resolve_binary_on_path(binary: &str, path_var: &OsStr) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
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

fn resolve_binary(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    resolve_binary_on_path(binary, &path_var)
}

/// A known preset found on the current PATH. Detection is stat-only: no CLI
/// process is started and nothing is registered until the caller explicitly
/// asks to persist the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedExternalAgent {
    pub name: String,
    pub command: PathBuf,
}

fn detect_installed_presets_on_path(path_var: &OsStr) -> Vec<DetectedExternalAgent> {
    builtin_profiles()
        .into_iter()
        .filter_map(|(name, binary, ..)| {
            resolve_binary_on_path(binary, path_var).map(|command| DetectedExternalAgent {
                name: name.to_string(),
                command,
            })
        })
        .collect()
}

/// Find installed built-in CLI presets without executing them. This is only
/// called by the explicit `/external-agents` command; engine startup continues
/// to build its registry exclusively from configuration.
pub fn detect_installed_presets() -> Vec<DetectedExternalAgent> {
    std::env::var_os("PATH")
        .map(|path| detect_installed_presets_on_path(&path))
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAgentRegistrationReport {
    pub detected: Vec<DetectedExternalAgent>,
    pub added: Vec<String>,
    pub already_registered: Vec<String>,
    pub config_changed: bool,
    pub effective_enabled: bool,
}

fn merge_detected_presets(
    global: &mut ExternalAgentsConfig,
    effective: &ExternalAgentsConfig,
    detected: &[DetectedExternalAgent],
) -> (Vec<String>, Vec<String>, bool) {
    let mut added = Vec::new();
    let mut already_registered = Vec::new();
    let mut changed = false;

    for item in detected {
        if effective.agents.contains_key(&item.name) {
            already_registered.push(item.name.clone());
        } else {
            global
                .agents
                .insert(item.name.clone(), ExternalAgentEntry::default());
            added.push(item.name.clone());
            changed = true;
        }
    }

    // An explicit discover/register request also reverses an old global
    // blanket disable. A project-level `enabled: false` still wins.
    if !detected.is_empty() && global.enabled == Some(false) {
        global.enabled = Some(true);
        changed = true;
    }

    (added, already_registered, changed)
}

/// Explicitly discover known CLIs and add missing presets to the global Zode
/// config. Existing global or project entries are never overwritten. The
/// config writer is atomic; callers may then rebuild their active engine.
pub fn detect_and_register_global(
    cwd: &Path,
) -> Result<ExternalAgentRegistrationReport, CoreError> {
    let detected = detect_installed_presets();
    let effective_before = ConfigManager::load(cwd)?;
    let mut global = ConfigManager::load_global()?;
    let (added, already_registered, config_changed) = merge_detected_presets(
        &mut global.external_agents,
        &effective_before.external_agents,
        &detected,
    );
    if config_changed {
        ConfigManager::save_global(&global)?;
    }
    let effective_enabled = if config_changed {
        ConfigManager::load(cwd)?.external_agents.enabled()
    } else {
        effective_before.external_agents.enabled()
    };
    Ok(ExternalAgentRegistrationReport {
        detected,
        added,
        already_registered,
        config_changed,
        effective_enabled,
    })
}

/// Resolve either an explicit path or a bare executable name. Bare names use
/// the same sanitized PATH lookup as presets; paths are canonicalized in
/// place. Resolution remains stat-only.
fn resolve_command(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || path.components().count() > 1 {
        return match path.canonicalize() {
            Ok(path) if is_executable_file(&path) => Some(path),
            _ => None,
        };
    }
    resolve_binary(command)
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

/// Build the registry exclusively from `externalAgents.agents`. Known names
/// use documented preset arguments/protocols, while arbitrary names are custom
/// profiles. `builtin_conflicts` carries names already taken by internal agent
/// types / user AgentDefs; collisions are disabled loudly.
pub fn discover(cfg: &ExternalAgentsConfig, builtin_conflicts: &[String]) -> ExternalAgentRegistry {
    let mut reg = ExternalAgentRegistry::default();
    if !cfg.enabled() {
        return reg;
    }

    let presets = builtin_profiles();
    for (name, entry) in &cfg.agents {
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

        if let Some((_, binary, capability, default_args, auth_env)) =
            presets.iter().find(|(preset, ..)| *preset == name)
        {
            let requested = entry.command.as_deref().unwrap_or(binary);
            let Some(command) = resolve_command(requested) else {
                reg.disabled.push((
                    name.clone(),
                    format!("configured command not found: {requested}"),
                ));
                continue;
            };
            let mut args = default_args.clone();
            if let Some(extra) = &entry.extra_args {
                args.extend(extra.clone());
            }
            reg.agents.push(ExternalAgentDef {
                name: name.clone(),
                command,
                args,
                capability: capability.clone(),
                auth_env: auth_env.clone(),
                env_allow: entry.env_allow.clone().unwrap_or_default(),
                trusted: entry.trusted.unwrap_or(false),
            });
            continue;
        }

        match def_from_entry(name, entry) {
            Ok(mut def) => match resolve_command(&def.command.to_string_lossy()) {
                Some(path) => {
                    def.command = path;
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
    fn registration_is_manual_and_resolution_is_stat_only() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, b"bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let empty = discover(&Default::default(), &[]);
        assert!(
            empty.is_empty(),
            "PATH contents must not auto-register CLIs"
        );

        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            "claude-code".into(),
            serde_json::from_value(serde_json::json!({
                "command": claude.display().to_string()
            }))
            .unwrap(),
        );
        let reg = discover(&cfg, &[]);
        assert!(reg.get("claude-code").is_some());
        assert!(reg.get("codex").is_none());
        assert!(!reg.agent_types().is_empty());
    }

    #[test]
    fn name_conflicts_disable_entry_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, b"bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            "claude-code".into(),
            serde_json::from_value(serde_json::json!({
                "command": claude.display().to_string()
            }))
            .unwrap(),
        );
        let reg = discover(&cfg, &["claude-code".to_string()]);
        assert!(reg.get("claude-code").is_none());
        assert!(reg
            .disabled()
            .iter()
            .any(|(n, r)| n == "claude-code" && r.contains("collides")));
    }

    #[test]
    fn custom_profile_accepts_a_bare_command_from_path() {
        let executable = resolve_binary("git").expect("git on PATH for repository tests");
        let mut cfg = ExternalAgentsConfig::default();
        cfg.agents.insert(
            "custom".into(),
            serde_json::from_value(serde_json::json!({
                "command": "git",
                "args": ["{prompt}"],
                "promptTransport": "argv"
            }))
            .unwrap(),
        );
        let reg = discover(&cfg, &[]);
        assert_eq!(
            reg.get("custom").map(|d| d.command.as_path()),
            Some(executable.as_path())
        );
    }

    #[test]
    fn explicit_detector_finds_presets_without_executing_them() {
        let dir = tempfile::tempdir().unwrap();
        let cursor = dir.path().join("cursor-agent");
        std::fs::write(&cursor, b"not a real executable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cursor, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = detect_installed_presets_on_path(dir.path().as_os_str());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "cursor");
        assert_eq!(found[0].command, cursor.canonicalize().unwrap());
    }

    #[test]
    fn registration_merge_adds_only_missing_presets() {
        let detected = vec![
            DetectedExternalAgent {
                name: "cursor".into(),
                command: PathBuf::from("/bin/cursor-agent"),
            },
            DetectedExternalAgent {
                name: "kiro".into(),
                command: PathBuf::from("/bin/kiro-cli"),
            },
        ];
        let mut global = ExternalAgentsConfig {
            enabled: Some(false),
            ..Default::default()
        };
        let mut effective = ExternalAgentsConfig::default();
        effective.agents.insert(
            "cursor".into(),
            serde_json::from_value(serde_json::json!({"enabled": false})).unwrap(),
        );

        let (added, existing, changed) = merge_detected_presets(&mut global, &effective, &detected);
        assert_eq!(added, vec!["kiro"]);
        assert_eq!(existing, vec!["cursor"]);
        assert!(changed);
        assert_eq!(global.enabled, Some(true));
        assert_eq!(
            global.agents.get("kiro"),
            Some(&ExternalAgentEntry::default())
        );
        assert!(!global.agents.contains_key("cursor"));
    }
}
