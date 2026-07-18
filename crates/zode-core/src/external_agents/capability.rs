//! Structured capability model for external agent profiles. A profile's
//! capability decides prompt delivery, output parsing, resume support, and
//! how its effective sandbox is presented in the trust approval view.

use std::fmt;

/// How the prompt reaches the CLI. `Argv` requires a `{prompt}` placeholder
/// in the args template; `File` requires `{prompt_file}` (the prompt is
/// written to a 0600 scratch file that is deleted after the run).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptTransport {
    Stdin,
    Argv,
    File,
}

/// How stdout is interpreted. `Text` accumulates raw stdout as the result;
/// the JSONL dialects map events (control-plane fields are mandatory there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputProtocol {
    Text,
    /// Generic newline-delimited JSON. Text and session IDs are selected with
    /// JSON pointers from the profile instead of a CLI-specific event schema.
    Jsonl,
    JsonlClaude,
    JsonlCodex,
}

/// The sandbox the external CLI applies to ITSELF, derived from the final
/// argv (extra args may relax it). `Unknown` renders worst-case in approvals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveSandbox {
    None,
    ReadOnly,
    WorkspaceWrite,
    Unrestricted,
    Unknown,
}

impl fmt::Display for EffectiveSandbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EffectiveSandbox::None => "none",
            EffectiveSandbox::ReadOnly => "read-only",
            EffectiveSandbox::WorkspaceWrite => "workspace-write",
            EffectiveSandbox::Unrestricted => "unrestricted",
            EffectiveSandbox::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileCapability {
    pub prompt_transport: PromptTransport,
    pub output_protocol: OutputProtocol,
    /// Simple resume flag (e.g. "--resume"). A profile is one-shot only when
    /// both this and `resume_args` are absent.
    pub resume_flag: Option<String>,
    /// General resume argv template, appended on later turns. Every
    /// `{session_id}` token is replaced with the captured session id. Takes
    /// precedence over `resume_flag`.
    pub resume_args: Option<Vec<String>>,
    /// Initial-run argv template for CLIs that accept a caller-selected
    /// session ID. Zode generates the ID and replaces `{session_id}`.
    pub new_session_args: Option<Vec<String>>,
    pub effective_sandbox: EffectiveSandbox,
    /// Minimal version accepted, compared against `--version` output AFTER
    /// the first trust approval (never before — running an untrusted binary
    /// is itself an approval-gated act).
    pub version_requirement: Option<String>,
    /// JSON pointer into the final result event (e.g. "/session_id").
    pub session_id_source: Option<String>,
    /// JSON pointer to streamed result text for the generic `jsonl` protocol.
    pub text_source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_sandbox_displays_lowercase() {
        assert_eq!(
            EffectiveSandbox::WorkspaceWrite.to_string(),
            "workspace-write"
        );
        assert_eq!(EffectiveSandbox::Unknown.to_string(), "unknown");
    }
}
