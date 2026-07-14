//! Interactive approval gate. Zode does its own per-call approval in a
//! tool decorator (see master plan §4.6①) because agent-rs 0.1.0's
//! QueryLoop does not pump the ExternalQueue — an `Ask` decision there
//! just synthesizes a failed ToolResult. So we keep the PermissionManager
//! carrying only hard-deny rules and gate interactively here.

use async_trait::async_trait;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

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

    /// Whether this gate can actually put a question to a HUMAN. Auto-answering
    /// gates (yolo / bypass) must say `false` — consent-style questions, like
    /// authorizing a sandbox escape, must never be "approved" by a gate that
    /// answers by itself. Defaults to `false` so a new gate is safe-by-default.
    fn interactive(&self) -> bool {
        false
    }
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
    fn interactive(&self) -> bool {
        true
    }

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
        "browser_act" => {
            let target = pick("_target");
            let detail = match pick("action") {
                "navigate" => pick("url").to_string(),
                "type" => format!("{} <- {:?}", pick("selector"), pick("text")),
                "key" => pick("key").to_string(),
                a @ ("click" | "scroll") => {
                    let _ = a;
                    input
                        .get("ref")
                        .map(|r| format!("ref {r}"))
                        .or_else(|| {
                            input
                                .get("selector")
                                .and_then(|s| s.as_str().map(String::from))
                        })
                        .unwrap_or_default()
                }
                _ => String::new(),
            };
            let t = if target.is_empty() {
                String::new()
            } else {
                format!(" [{target}]")
            };
            format!("{} {}{}", pick("action"), detail, t)
                .trim()
                .to_string()
        }
        "browser_eval" => {
            let t = pick("_target");
            let t = if t.is_empty() {
                String::new()
            } else {
                format!(" [{t}]")
            };
            format!("evaluate JS{t}: {}", pick("expression"))
        }
        "browser_tabs" => format!("{} tab {}", pick("action"), pick("id"))
            .trim()
            .to_string(),
        "browser_upload" => {
            let target = pick("_target");
            let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let files = input
                .get("files")
                .and_then(|v| v.as_array())
                .map(|files| {
                    files
                        .iter()
                        .map(|file| {
                            format!(
                                "{} ({} bytes)",
                                file.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                                file.get("size").and_then(|v| v.as_u64()).unwrap_or(0)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("upload {count} file(s) [{target}]: {files}")
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
    /// Turn bound to `source` when this request entered the queue. This is a
    /// snapshot: rebinding the source for a later turn cannot retag an older
    /// pending approval.
    pub turn_id: Option<u64>,
    /// Local operation bound to `source` when this request entered the queue.
    /// This is mutually exclusive with [`Self::turn_id`], so equal numeric
    /// generations from the two domains can never be confused.
    pub local_op_id: Option<u64>,
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
    owner_by_source: Arc<Mutex<HashMap<String, ApprovalOwner>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalOwner {
    Turn(u64),
    LocalOperation(u64),
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
        ApprovalQueue {
            sender: tx,
            owner_by_source: Arc::new(Mutex::new(HashMap::new())),
        },
        ApprovalReceiver { receiver: rx },
    )
}

impl ApprovalQueue {
    /// Bind future requests from `source` to `turn_id`. Zero is not a valid
    /// turn and clears any older binding so an invalid transition fails closed.
    pub fn bind_turn(&self, source: &str, turn_id: u64) {
        let Ok(mut owners) = self.owner_by_source.lock() else {
            return;
        };
        if source.is_empty() || turn_id == 0 {
            owners.remove(source);
        } else {
            owners.insert(source.to_owned(), ApprovalOwner::Turn(turn_id));
        }
    }

    /// Bind future requests from `source` to a local operation. This replaces
    /// any turn binding for the source so ownership remains type-exclusive.
    pub fn bind_local_operation(&self, source: &str, local_op_id: u64) {
        let Ok(mut owners) = self.owner_by_source.lock() else {
            return;
        };
        if source.is_empty() || local_op_id == 0 {
            owners.remove(source);
        } else {
            owners.insert(
                source.to_owned(),
                ApprovalOwner::LocalOperation(local_op_id),
            );
        }
    }

    /// Remove `source` only while it still names `expected`. A delayed terminal
    /// for an older turn therefore cannot clear a newer turn's binding.
    pub fn clear_turn_if(&self, source: &str, expected: u64) {
        let Ok(mut owners) = self.owner_by_source.lock() else {
            return;
        };
        if owners.get(source).copied() == Some(ApprovalOwner::Turn(expected)) {
            owners.remove(source);
        }
    }

    /// Remove `source` only while it still names the expected local operation.
    pub fn clear_local_operation_if(&self, source: &str, expected: u64) {
        let Ok(mut owners) = self.owner_by_source.lock() else {
            return;
        };
        if owners.get(source).copied() == Some(ApprovalOwner::LocalOperation(expected)) {
            owners.remove(source);
        }
    }

    /// Remove all turn ownership for a source (for example, a closed tab).
    pub fn remove_source(&self, source: &str) {
        let Ok(mut owners) = self.owner_by_source.lock() else {
            return;
        };
        owners.remove(source);
    }

    fn owner_for_source(&self, source: Option<&str>) -> (Option<u64>, Option<u64>) {
        let Some(source) = source.filter(|source| !source.is_empty()) else {
            return (None, None);
        };
        match self
            .owner_by_source
            .lock()
            .ok()
            .and_then(|owners| owners.get(source).copied())
        {
            Some(ApprovalOwner::Turn(turn_id)) if turn_id > 0 => (Some(turn_id), None),
            Some(ApprovalOwner::LocalOperation(local_op_id)) if local_op_id > 0 => {
                (None, Some(local_op_id))
            }
            _ => (None, None),
        }
    }

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
        let (turn_id, local_op_id) = self.owner_for_source(source.as_deref());
        let req = ApprovalRequest {
            tool: tool.to_string(),
            input: input.clone(),
            source,
            turn_id,
            local_op_id,
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
    fn interactive(&self) -> bool {
        true
    }

    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        self.queue.request(tool, input, self.label.clone()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spawn_request(
        queue: ApprovalQueue,
        source: Option<&str>,
    ) -> tokio::task::JoinHandle<Approval> {
        let source = source.map(str::to_owned);
        tokio::spawn(async move { queue.request("Bash", &json!({}), source).await })
    }

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

    #[tokio::test]
    async fn queued_request_keeps_turn_snapshot_after_rebind() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 1);

        let pending = spawn_request(queue.clone(), Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        queue.bind_turn("tab-1", 2);

        assert_eq!(request.turn_id, Some(1));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn queue_clones_share_turn_bindings() {
        let (queue, mut rx) = approval_queue();
        let clone = queue.clone();
        clone.bind_turn("tab-1", 7);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, Some(7));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn local_operation_binding_is_shared_and_exclusive_with_turn_binding() {
        let (queue, mut rx) = approval_queue();
        let clone = queue.clone();
        clone.bind_local_operation("tab-1", 7);

        let pending = spawn_request(queue.clone(), Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(7));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        queue.bind_turn("tab-1", 8);
        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, Some(8));
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn clear_turn_if_preserves_newer_binding() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 1);
        queue.bind_turn("tab-1", 2);
        queue.clear_turn_if("tab-1", 1);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, Some(2));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn clear_turn_if_removes_matching_binding() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 2);
        queue.clear_turn_if("tab-1", 2);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn clear_local_operation_if_preserves_newer_binding() {
        let (queue, mut rx) = approval_queue();
        queue.bind_local_operation("tab-1", 1);
        queue.bind_local_operation("tab-1", 2);
        queue.clear_local_operation_if("tab-1", 1);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(2));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn typed_exact_clear_does_not_cross_turn_and_local_operation_owners() {
        let (queue, mut rx) = approval_queue();
        queue.bind_local_operation("tab-1", 9);
        queue.clear_turn_if("tab-1", 9);

        let pending = spawn_request(queue.clone(), Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(9));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        queue.bind_turn("tab-1", 9);
        queue.clear_local_operation_if("tab-1", 9);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");
        assert_eq!(request.turn_id, Some(9));
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn remove_source_clears_turn_binding() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 3);
        queue.remove_source("tab-1");

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn unbound_source_has_no_turn_id() {
        let (queue, mut rx) = approval_queue();

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn source_less_request_has_no_turn_id() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 4);

        let pending = spawn_request(queue, None);
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn zero_turn_id_fails_closed_without_a_binding() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 5);
        queue.bind_turn("tab-1", 0);

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn poisoned_turn_registry_fails_closed_without_a_turn_id() {
        let (queue, mut rx) = approval_queue();
        queue.bind_turn("tab-1", 6);
        let owner_by_source = queue.owner_by_source.clone();
        let poisoned = std::thread::spawn(move || {
            let _guard = owner_by_source.lock().unwrap();
            panic!("poison turn registry");
        });
        assert!(poisoned.join().is_err());

        let pending = spawn_request(queue, Some("tab-1"));
        let request = rx.next().await.expect("request should be queued");

        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[test]
    fn summarize_browser_tools() {
        let s = summarize_input(
            "browser_act",
            &json!({"action":"navigate","url":"https://x.test","_target":"managed"}),
        );
        assert_eq!(s, "navigate https://x.test [managed]");
        let s = summarize_input(
            "browser_eval",
            &json!({"expression":"document.title","_target":"managed"}),
        );
        assert_eq!(s, "evaluate JS [managed]: document.title");
        let s = summarize_input("browser_tabs", &json!({"action":"close","id":"t1"}));
        assert_eq!(s, "close tab t1");
    }
}
