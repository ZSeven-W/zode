//! Built-in and custom external agent profiles. Built-in flags live here so
//! CLI drift is corrected in exactly one place; custom profiles come from
//! `externalAgents.agents` config entries and are validated in
//! [`def_from_entry`].

use std::path::PathBuf;

use crate::config::ExternalAgentEntry;

use super::capability::{EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport};

/// A fully resolved external agent: what to run and how to talk to it.
/// `command` is canonicalized at discovery time (stat-only, never executed).
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalAgentDef {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub capability: ProfileCapability,
    pub auth_env: Vec<String>,
    pub env_allow: Vec<String>,
    pub trusted: bool,
}

/// `(agent_type, binary, capability, args_template, auth_env)`.
pub type BuiltinProfile = (
    &'static str,
    &'static str,
    ProfileCapability,
    Vec<String>,
    Vec<String>,
);

/// Built-in profiles. Flags verified against installed CLIs during
/// implementation; adjust here only.
pub fn builtin_profiles() -> Vec<BuiltinProfile> {
    vec![
        (
            "claude-code",
            "claude",
            ProfileCapability {
                prompt_transport: PromptTransport::Stdin,
                output_protocol: OutputProtocol::JsonlClaude,
                resume_flag: Some("--resume".to_string()),
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/session_id".to_string()),
            },
            vec![
                "-p".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            vec![],
        ),
        (
            "codex",
            "codex",
            ProfileCapability {
                prompt_transport: PromptTransport::Stdin,
                output_protocol: OutputProtocol::JsonlCodex,
                resume_flag: Some("resume".to_string()),
                effective_sandbox: EffectiveSandbox::WorkspaceWrite,
                version_requirement: None,
                session_id_source: Some("/session_id".to_string()),
            },
            vec![
                "exec".to_string(),
                "--json".to_string(),
                // Current flag (replaces the deprecated `--full-auto`); grants
                // codex its workspace-write self-sandbox.
                "--sandbox".to_string(),
                "workspace-write".to_string(),
            ],
            vec![],
        ),
        (
            "opencode",
            "opencode",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Text,
                resume_flag: None,
                effective_sandbox: EffectiveSandbox::Unknown,
                version_requirement: None,
                session_id_source: None,
            },
            vec!["run".to_string(), "{prompt}".to_string()],
            vec![],
        ),
    ]
}

/// Build a custom-profile def from a config entry. Validates the transport /
/// placeholder contract so an unusable profile fails at config load, not at
/// spawn time. The returned `command` is NOT canonicalized here — discovery
/// owns path resolution.
pub fn def_from_entry(name: &str, entry: &ExternalAgentEntry) -> Result<ExternalAgentDef, String> {
    let command = entry
        .command
        .as_deref()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| format!("external agent '{name}': missing \"command\""))?;
    let args = entry.args.clone().unwrap_or_default();

    let prompt_transport = match entry.prompt_transport.as_deref().unwrap_or("argv") {
        "stdin" => PromptTransport::Stdin,
        "argv" => PromptTransport::Argv,
        "file" => PromptTransport::File,
        other => {
            return Err(format!(
                "external agent '{name}': unknown promptTransport \"{other}\""
            ));
        }
    };
    match prompt_transport {
        PromptTransport::Argv if !args.iter().any(|a| a == "{prompt}") => {
            return Err(format!(
                "external agent '{name}': promptTransport \"argv\" requires a \"{{prompt}}\" placeholder in args"
            ));
        }
        PromptTransport::File if !args.iter().any(|a| a == "{prompt_file}") => {
            return Err(format!(
                "external agent '{name}': promptTransport \"file\" requires a \"{{prompt_file}}\" placeholder in args"
            ));
        }
        _ => {}
    }

    let output_protocol = match entry.output.as_deref().unwrap_or("text") {
        "text" => OutputProtocol::Text,
        "jsonl-claude" => OutputProtocol::JsonlClaude,
        "jsonl-codex" => OutputProtocol::JsonlCodex,
        other => {
            return Err(format!(
                "external agent '{name}': unknown output \"{other}\""
            ));
        }
    };

    let effective_sandbox = match entry.effective_sandbox.as_deref().unwrap_or("unknown") {
        "none" => EffectiveSandbox::None,
        "readOnly" => EffectiveSandbox::ReadOnly,
        "workspaceWrite" => EffectiveSandbox::WorkspaceWrite,
        "unrestricted" => EffectiveSandbox::Unrestricted,
        "unknown" => EffectiveSandbox::Unknown,
        other => {
            return Err(format!(
                "external agent '{name}': unknown effectiveSandbox \"{other}\""
            ));
        }
    };

    Ok(ExternalAgentDef {
        name: name.to_string(),
        command: PathBuf::from(command),
        args,
        capability: ProfileCapability {
            prompt_transport,
            output_protocol,
            resume_flag: entry.resume_flag.clone(),
            effective_sandbox,
            version_requirement: entry.version_requirement.clone(),
            session_id_source: entry.session_id_source.clone(),
        },
        auth_env: entry.auth_env.clone().unwrap_or_default(),
        env_allow: entry.env_allow.clone().unwrap_or_default(),
        trusted: entry.trusted.unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_argv_profile_requires_prompt_placeholder() {
        let entry: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"my-agent","args":["run"],"promptTransport":"argv","output":"text"}"#,
        )
        .unwrap();
        let err = def_from_entry("my-cli", &entry).unwrap_err();
        assert!(err.contains("{prompt}"), "err: {err}");

        let ok: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"my-agent","args":["run","{prompt}"],"promptTransport":"argv","output":"text"}"#,
        )
        .unwrap();
        let def = def_from_entry("my-cli", &ok).unwrap();
        assert!(matches!(
            def.capability.prompt_transport,
            PromptTransport::Argv
        ));
        assert!(matches!(
            def.capability.output_protocol,
            OutputProtocol::Text
        ));
        assert!(!def.trusted);
    }

    #[test]
    fn file_transport_requires_prompt_file_placeholder() {
        let entry: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"my-agent","args":["run"],"promptTransport":"file"}"#,
        )
        .unwrap();
        assert!(def_from_entry("f", &entry)
            .unwrap_err()
            .contains("{prompt_file}"));
    }

    #[test]
    fn unknown_enum_values_are_rejected() {
        let entry: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"x","args":["{prompt}"],"promptTransport":"telepathy"}"#,
        )
        .unwrap();
        assert!(def_from_entry("t", &entry).is_err());
    }

    #[test]
    fn builtin_profiles_cover_three_clis() {
        let names: Vec<_> = builtin_profiles().into_iter().map(|p| p.0).collect();
        assert_eq!(names, vec!["claude-code", "codex", "opencode"]);
    }
}
