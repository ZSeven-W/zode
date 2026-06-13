//! Interactive approval gate. Zode does its own per-call approval in a
//! tool decorator (see master plan §4.6①) because agent-rs 0.1.0's
//! QueryLoop does not pump the ExternalQueue — an `Ask` decision there
//! just synthesizes a failed ToolResult. So we keep the PermissionManager
//! carrying only hard-deny rules and gate interactively here.

use async_trait::async_trait;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// Host-supplied approver. Headless uses [`StdinGate`]; the TUI supplies
/// a queue-backed gate (Phase 05); `--yolo` uses [`BypassGate`].
#[async_trait]
pub trait ApprovalGate: Send + Sync + std::fmt::Debug {
    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval;
}

#[derive(Debug)]
pub struct BypassGate;

#[async_trait]
impl ApprovalGate for BypassGate {
    async fn approve(&self, _tool: &str, _input: &serde_json::Value) -> Approval {
        Approval::AllowOnce
    }
}

#[derive(Debug)]
pub struct StdinGate;

#[async_trait]
impl ApprovalGate for StdinGate {
    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        // Render the prompt on a blocking thread so we don't fight the
        // async reactor for stdin.
        let tool = tool.to_string();
        let summary = summarize_input(&tool, input);
        tokio::task::spawn_blocking(move || {
            let mut out = std::io::stderr();
            let _ = writeln!(out, "\n⚠ Tool '{tool}' wants to run:");
            let _ = writeln!(out, "  {summary}");
            let _ = write!(out, "  Allow? [y]es / [a]lways / [N]o: ");
            let _ = out.flush();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                return Approval::Deny;
            }
            parse_answer(line.trim()).unwrap_or(Approval::Deny)
        })
        .await
        .unwrap_or(Approval::Deny)
    }
}

pub(crate) fn parse_answer(s: &str) -> Option<Approval> {
    match s.trim().to_lowercase().as_str() {
        "y" | "yes" => Some(Approval::AllowOnce),
        "a" | "always" => Some(Approval::AllowAlways),
        "n" | "no" | "" => Some(Approval::Deny),
        _ => None,
    }
}

/// One-line summary of a tool call for the approval prompt.
pub(crate) fn summarize_input(tool: &str, input: &serde_json::Value) -> String {
    let pick = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
    match tool {
        "Bash" | "BashRun" => format!("$ {}", pick("command")),
        "FileWrite" | "FileEdit" | "Remove" | "Move" | "Mkdir" => {
            format!("{tool} {}", pick("path"))
        }
        _ => {
            let compact = serde_json::to_string(input).unwrap_or_default();
            let max = 120;
            if compact.len() > max {
                format!("{tool} {}…", &compact[..max])
            } else {
                format!("{tool} {compact}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn bypass_gate_always_allows() {
        let g = BypassGate;
        assert_eq!(
            g.approve("Bash", &json!({"command": "ls"})).await,
            Approval::AllowOnce
        );
    }

    #[test]
    fn parse_stdin_answer_maps_keys() {
        assert_eq!(parse_answer("y"), Some(Approval::AllowOnce));
        assert_eq!(parse_answer("a"), Some(Approval::AllowAlways));
        assert_eq!(parse_answer("n"), Some(Approval::Deny));
        assert_eq!(parse_answer(""), Some(Approval::Deny));
        assert_eq!(parse_answer("garbage"), None);
    }

    #[test]
    fn summary_truncates_long_input() {
        let big = json!({"data": "x".repeat(500)});
        let s = summarize_input("Weird", &big);
        assert!(s.len() < 200);
        assert!(s.ends_with('…'));
    }
}
