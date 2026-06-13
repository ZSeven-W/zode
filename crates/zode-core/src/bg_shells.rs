//! Host-side tracking of background shells. agent-tools-code's
//! BashSessionRegistry only exposes a count, so Zode captures shell_id +
//! command from BashRun's ToolResult via a hook to power the task panel
//! (Phase 07). Kill/output still go through the KillShell/BashOutput tools
//! by shell_id.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::hook::{HookEvent, HookHandler, HookOutcome};
use async_trait::async_trait;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct BgShell {
    pub shell_id: String,
    pub command: String,
    pub started_at: u64,
    pub killed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BackgroundShellTracker {
    inner: Arc<Mutex<Vec<BgShell>>>,
}

impl BackgroundShellTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record(&self, shell_id: String, command: String, started_at: u64) {
        self.inner.lock().await.push(BgShell {
            shell_id,
            command,
            started_at,
            killed: false,
        });
    }

    pub async fn mark_killed(&self, shell_id: &str) {
        if let Some(s) = self
            .inner
            .lock()
            .await
            .iter_mut()
            .find(|s| s.shell_id == shell_id)
        {
            s.killed = true;
        }
    }

    pub async fn list(&self) -> Vec<BgShell> {
        self.inner.lock().await.clone()
    }
}

/// Pull the shell id out of BashRun's output JSON (field `shell_id`).
pub(crate) fn shell_id_of(output: &serde_json::Value) -> Option<String> {
    output
        .get("shell_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Hook that records BashRun launches and KillShell kills.
#[derive(Debug)]
pub struct BgShellHook {
    tracker: BackgroundShellTracker,
}

impl BgShellHook {
    pub fn new(tracker: BackgroundShellTracker) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl HookHandler for BgShellHook {
    async fn handle(&self, event: &HookEvent) -> HookOutcome {
        if let HookEvent::AfterToolUse {
            tool,
            input,
            output,
            ok,
        } = event
        {
            if *ok && tool == "BashRun" {
                if let Some(id) = shell_id_of(output) {
                    // command is in both input and output; input is canonical.
                    let cmd = str_field(input, "command");
                    self.tracker.record(id, cmd, now_secs()).await;
                }
            } else if *ok && tool == "KillShell" {
                let id = str_field(input, "shell_id");
                if !id.is_empty() {
                    self.tracker.mark_killed(&id).await;
                }
            }
        }
        HookOutcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn tracks_then_kills() {
        let t = BackgroundShellTracker::new();
        t.record("bash_1".into(), "sleep 100".into(), 1000).await;
        t.record("bash_2".into(), "tail -f log".into(), 1001).await;
        assert_eq!(t.list().await.len(), 2);
        t.mark_killed("bash_1").await;
        let list = t.list().await;
        assert!(list.iter().find(|s| s.shell_id == "bash_1").unwrap().killed);
        assert!(!list.iter().find(|s| s.shell_id == "bash_2").unwrap().killed);
    }

    #[test]
    fn extract_shell_id() {
        assert_eq!(
            shell_id_of(&json!({"shell_id": "bash_42", "status": "running"})),
            Some("bash_42".to_string())
        );
        assert_eq!(shell_id_of(&json!({})), None);
    }

    #[tokio::test]
    async fn hook_records_bashrun_and_marks_kill() {
        let tracker = BackgroundShellTracker::new();
        let hook = BgShellHook::new(tracker.clone());
        hook.handle(&HookEvent::AfterToolUse {
            tool: "BashRun".into(),
            input: json!({"command": "sleep 99"}),
            output: json!({"shell_id": "bash_x", "command": "sleep 99"}),
            ok: true,
        })
        .await;
        assert_eq!(tracker.list().await.len(), 1);
        assert_eq!(tracker.list().await[0].command, "sleep 99");

        hook.handle(&HookEvent::AfterToolUse {
            tool: "KillShell".into(),
            input: json!({"shell_id": "bash_x"}),
            output: json!({}),
            ok: true,
        })
        .await;
        assert!(tracker.list().await[0].killed);
    }
}
