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

/// Built-in manual presets. Keep documented headless flags centralized here
/// so CLI drift is corrected in exactly one place.
pub fn builtin_profiles() -> Vec<BuiltinProfile> {
    vec![
        (
            "claude-code",
            "claude",
            ProfileCapability {
                prompt_transport: PromptTransport::Stdin,
                output_protocol: OutputProtocol::JsonlClaude,
                resume_flag: Some("--resume".to_string()),
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/session_id".to_string()),
                text_source: None,
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
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::WorkspaceWrite,
                version_requirement: None,
                session_id_source: Some("/session_id".to_string()),
                text_source: None,
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
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unknown,
                version_requirement: None,
                session_id_source: None,
                text_source: None,
            },
            vec!["run".to_string(), "{prompt}".to_string()],
            vec![],
        ),
        (
            "cline",
            "cline",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Jsonl,
                resume_flag: None,
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: None,
                text_source: Some("/text".to_string()),
            },
            vec!["--json".to_string(), "{prompt}".to_string()],
            vec![],
        ),
        (
            "antigravity",
            "agy",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Text,
                resume_flag: None,
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unknown,
                version_requirement: None,
                session_id_source: None,
                text_source: None,
            },
            vec!["-p".to_string(), "{prompt}".to_string()],
            vec![],
        ),
        (
            "cursor",
            "cursor-agent",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Jsonl,
                resume_flag: None,
                resume_args: Some(vec!["--resume".to_string(), "{session_id}".to_string()]),
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/session_id".to_string()),
                text_source: Some("/result".to_string()),
            },
            vec![
                "--print".to_string(),
                "--force".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "{prompt}".to_string(),
            ],
            vec!["CURSOR_API_KEY".to_string()],
        ),
        (
            "kiro",
            "kiro-cli",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Text,
                resume_flag: None,
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: None,
                text_source: None,
            },
            vec![
                "chat".to_string(),
                "--no-interactive".to_string(),
                "--trust-all-tools".to_string(),
                "{prompt}".to_string(),
            ],
            vec!["KIRO_API_KEY".to_string()],
        ),
        (
            "pi",
            "pi",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Jsonl,
                resume_flag: None,
                resume_args: Some(vec!["--session".to_string(), "{session_id}".to_string()]),
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/id".to_string()),
                // Pi's JSON stream starts with {type:"session",id:...} and
                // emits complete assistant messages on message_end events.
                text_source: Some("/message/content/0/text".to_string()),
            },
            vec![
                "--mode".to_string(),
                "json".to_string(),
                "{prompt}".to_string(),
            ],
            vec![],
        ),
        (
            "grok",
            "grok",
            ProfileCapability {
                prompt_transport: PromptTransport::Argv,
                output_protocol: OutputProtocol::Text,
                resume_flag: None,
                resume_args: Some(vec!["--resume".to_string(), "{session_id}".to_string()]),
                new_session_args: Some(vec![
                    "--session-id".to_string(),
                    "{session_id}".to_string(),
                ]),
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: None,
                text_source: None,
            },
            vec![
                "--no-auto-update".to_string(),
                "-p".to_string(),
                "{prompt}".to_string(),
                "--output-format".to_string(),
                "plain".to_string(),
                "--always-approve".to_string(),
            ],
            vec!["XAI_API_KEY".to_string()],
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
        "jsonl" => OutputProtocol::Jsonl,
        "jsonl-claude" => OutputProtocol::JsonlClaude,
        "jsonl-codex" => OutputProtocol::JsonlCodex,
        other => {
            return Err(format!(
                "external agent '{name}': unknown output \"{other}\""
            ));
        }
    };

    if let Some(args) = &entry.resume_args {
        if !args.iter().any(|arg| arg == "{session_id}") {
            return Err(format!(
                "external agent '{name}': resumeArgs requires a \"{{session_id}}\" token"
            ));
        }
    }
    if let Some(args) = &entry.new_session_args {
        if !args.iter().any(|arg| arg == "{session_id}") {
            return Err(format!(
                "external agent '{name}': newSessionArgs requires a \"{{session_id}}\" token"
            ));
        }
    }
    let supports_resume = entry.resume_flag.is_some() || entry.resume_args.is_some();
    if entry.new_session_args.is_some() && !supports_resume {
        return Err(format!(
            "external agent '{name}': newSessionArgs requires resumeArgs or resumeFlag"
        ));
    }
    if supports_resume && entry.new_session_args.is_none() {
        match output_protocol {
            OutputProtocol::Text => {
                return Err(format!(
                    "external agent '{name}': resumable profiles require a JSONL output protocol"
                ));
            }
            OutputProtocol::Jsonl if entry.session_id_source.is_none() => {
                return Err(format!(
                    "external agent '{name}': resumable generic JSONL requires sessionIdSource"
                ));
            }
            OutputProtocol::Jsonl | OutputProtocol::JsonlClaude | OutputProtocol::JsonlCodex => {}
        }
    }

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
            resume_args: entry.resume_args.clone(),
            new_session_args: entry.new_session_args.clone(),
            effective_sandbox,
            version_requirement: entry.version_requirement.clone(),
            session_id_source: entry.session_id_source.clone(),
            text_source: entry.text_source.clone(),
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
    fn builtin_profiles_cover_manual_cli_presets() {
        let profiles = builtin_profiles();
        let names: Vec<_> = profiles.iter().map(|p| p.0).collect();
        assert_eq!(
            names,
            vec![
                "claude-code",
                "codex",
                "opencode",
                "cline",
                "antigravity",
                "cursor",
                "kiro",
                "pi",
                "grok"
            ]
        );
        let pi = profiles.iter().find(|p| p.0 == "pi").unwrap();
        assert!(matches!(pi.2.output_protocol, OutputProtocol::Jsonl));
        assert_eq!(pi.2.session_id_source.as_deref(), Some("/id"));
        assert!(pi.2.resume_args.is_some());
        let grok = profiles.iter().find(|p| p.0 == "grok").unwrap();
        assert!(matches!(grok.2.output_protocol, OutputProtocol::Text));
        assert!(grok.2.new_session_args.is_some());
        assert!(grok.2.resume_args.is_some());
        let cursor = profiles.iter().find(|p| p.0 == "cursor").unwrap();
        assert!(matches!(cursor.2.output_protocol, OutputProtocol::Jsonl));
        assert_eq!(cursor.2.session_id_source.as_deref(), Some("/session_id"));
        assert_eq!(cursor.2.text_source.as_deref(), Some("/result"));
        assert!(cursor.2.resume_args.is_some());
        assert!(cursor.3.iter().any(|arg| arg == "--force"));
    }

    #[test]
    fn generic_jsonl_and_resume_args_are_validated() {
        let ok: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"any-agent","args":["{prompt}"],"output":"jsonl",
                "textSource":"/delta","sessionIdSource":"/session/id",
                "resumeArgs":["--session","{session_id}"]}"#,
        )
        .unwrap();
        let def = def_from_entry("any", &ok).unwrap();
        assert!(matches!(
            def.capability.output_protocol,
            OutputProtocol::Jsonl
        ));
        assert_eq!(def.capability.text_source.as_deref(), Some("/delta"));

        let bad: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"x","args":["{prompt}"],"resumeArgs":["--resume"]}"#,
        )
        .unwrap();
        assert!(def_from_entry("bad", &bad)
            .unwrap_err()
            .contains("{session_id}"));

        let missing_source: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"x","args":["{prompt}"],"output":"jsonl",
                "resumeArgs":["--resume","{session_id}"]}"#,
        )
        .unwrap();
        assert!(def_from_entry("missing-source", &missing_source)
            .unwrap_err()
            .contains("sessionIdSource"));

        let text_resume: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"x","args":["{prompt}"],"output":"text",
                "resumeFlag":"--resume"}"#,
        )
        .unwrap();
        assert!(def_from_entry("text-resume", &text_resume)
            .unwrap_err()
            .contains("JSONL"));

        let host_session: ExternalAgentEntry = serde_json::from_str(
            r#"{"command":"x","args":["{prompt}"],"output":"text",
                "newSessionArgs":["--session-id","{session_id}"],
                "resumeArgs":["--resume","{session_id}"]}"#,
        )
        .unwrap();
        assert!(def_from_entry("host-session", &host_session).is_ok());
    }
}
