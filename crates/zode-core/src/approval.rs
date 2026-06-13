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

#[derive(Debug, Default)]
pub struct StdinGate {
    /// Serializes prompts: QueryLoop can dispatch tools concurrently, and
    /// two unsynchronized stdin reads would interleave prompts and steal
    /// each other's answers. One prompt is active at a time.
    prompt_lock: tokio::sync::Mutex<()>,
}

impl StdinGate {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ApprovalGate for StdinGate {
    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        // Hold the prompt lock across the whole prompt+read so concurrent
        // approvals queue instead of interleaving on the terminal.
        let _serialize = self.prompt_lock.lock().await;
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
            let max_chars = 120;
            // Truncate on a char boundary — byte slicing JSON with
            // non-ASCII content can split a UTF-8 code point and panic.
            if compact.chars().count() > max_chars {
                let truncated: String = compact.chars().take(max_chars).collect();
                format!("{tool} {truncated}…")
            } else {
                format!("{tool} {compact}")
            }
        }
    }
}

// ---------------------------------------------------------------------
// ApprovalQueue — the agent↔UI bridge for the TUI's QueueGate.
//
// Mirrors agent's external_queue.rs request/respond/timeout pattern, but
// carries Zode's three-state `Approval` (agent's ExternalQueue is locked to
// PermissionDecision, which can't express AllowAlways — master §4.6①).
// ---------------------------------------------------------------------

use tokio::sync::{mpsc, oneshot};

/// One pending approval request flowing from a gated tool to the UI.
#[derive(Debug)]
pub struct ApprovalRequest {
    pub tool: String,
    pub input: serde_json::Value,
    /// Opaque label identifying the source (the TUI sets this to the
    /// requesting tab's id) so the UI can focus the right conversation.
    pub source: Option<String>,
    sender: oneshot::Sender<Approval>,
}

impl ApprovalRequest {
    /// Send the user's decision back to the waiting tool. Err(approval) if
    /// the requester already gave up (rare — turn aborted).
    pub fn respond(self, approval: Approval) -> Result<(), Approval> {
        self.sender.send(approval)
    }

    /// One-line summary for the dialog.
    pub fn summary(&self) -> String {
        summarize_input(&self.tool, &self.input)
    }
}

/// Tool-facing handle (cheap to clone).
#[derive(Debug, Clone)]
pub struct ApprovalQueue {
    sender: mpsc::UnboundedSender<ApprovalRequest>,
}

/// UI-facing handle — single consumer; drain in the TUI select! loop.
#[derive(Debug)]
pub struct ApprovalReceiver {
    receiver: mpsc::UnboundedReceiver<ApprovalRequest>,
}

impl ApprovalReceiver {
    pub async fn next(&mut self) -> Option<ApprovalRequest> {
        self.receiver.recv().await
    }
}

pub fn approval_queue() -> (ApprovalQueue, ApprovalReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        ApprovalQueue { sender: tx },
        ApprovalReceiver { receiver: rx },
    )
}

impl ApprovalQueue {
    /// Submit a request and await the user's decision. A closed queue (no
    /// UI draining) or a dropped responder fails closed -> Deny. `source`
    /// labels the requester (the TUI tab id) so the UI can focus it.
    pub async fn request(
        &self,
        tool: &str,
        input: &serde_json::Value,
        source: Option<String>,
    ) -> Approval {
        let (tx, rx) = oneshot::channel();
        let req = ApprovalRequest {
            tool: tool.to_string(),
            input: input.clone(),
            source,
            sender: tx,
        };
        if self.sender.send(req).is_err() {
            return Approval::Deny;
        }
        rx.await.unwrap_or(Approval::Deny)
    }
}

/// ApprovalGate backed by the queue (used by the TUI). Each tab's engine gets
/// a gate labeled with that tab's id so approvals carry their source.
#[derive(Debug)]
pub struct QueueGate {
    queue: ApprovalQueue,
    label: Option<String>,
}

impl QueueGate {
    pub fn new(queue: ApprovalQueue) -> Self {
        Self { queue, label: None }
    }

    /// Gate whose requests are tagged with `label` (the source tab id).
    pub fn with_label(queue: ApprovalQueue, label: Option<String>) -> Self {
        Self { queue, label }
    }
}

#[async_trait]
impl ApprovalGate for QueueGate {
    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        self.queue.request(tool, input, self.label.clone()).await
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
        assert!(s.chars().count() < 200);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn summary_truncation_is_utf8_safe() {
        // Multibyte content whose byte length crosses the cap must not
        // panic on a non-char-boundary slice.
        let big = json!({"data": "你".repeat(500)});
        let s = summarize_input("Weird", &big); // must not panic
        assert!(s.ends_with('…'));
    }

    #[tokio::test]
    async fn approval_queue_round_trip() {
        let (queue, mut rx) = approval_queue();
        tokio::spawn(async move {
            while let Some(req) = rx.next().await {
                let a = if req.tool == "Bash" {
                    Approval::AllowOnce
                } else {
                    Approval::Deny
                };
                let _ = req.respond(a);
            }
        });
        let gate = QueueGate::new(queue);
        assert_eq!(
            gate.approve("Bash", &json!({"command": "ls"})).await,
            Approval::AllowOnce
        );
        assert_eq!(gate.approve("Other", &json!({})).await, Approval::Deny);
    }

    #[tokio::test]
    async fn approval_queue_closed_defaults_deny() {
        let (queue, rx) = approval_queue();
        drop(rx);
        let gate = QueueGate::new(queue);
        assert_eq!(gate.approve("Bash", &json!({})).await, Approval::Deny);
    }
}
