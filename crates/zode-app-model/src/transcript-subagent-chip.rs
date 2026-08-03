//! Inline sub-agent lifecycle chips (Codex parity: "started working" /
//! "made progress" / "finished").
//!
//! One compact line per lifecycle step of one `Task`-spawned sub-agent, shown
//! inline in the conversation. Chips are deliberately one-liners rather than
//! cards: the full picture (tokens, elapsed time, paging over finished
//! agents) lives in the sub-agent panel, and duplicating it here would turn
//! a busy delegation into wall-to-wall furniture.
//!
//! Chips are derived from `AgentEventKind::SubagentUpdate`, whose producer
//! already coalesces token-only churn into at most one event per second (see
//! `EventNormalizer::diff_subagents` in zode-app-runtime). The reducer adds
//! the other half of the contract - at most one live `Progress` chip per
//! agent, rewritten in place rather than stacked - in
//! `reducer/subagent-chips.rs`.

use zode_node_protocol::{SubagentSnapshot, SubagentStatus};

/// Which lifecycle step one chip reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentChipPhase {
    Started,
    Progress,
    Finished,
    Failed,
}

impl SubagentChipPhase {
    /// The terminal phase for a sub-agent status, or `None` while it runs.
    pub const fn from_status(status: SubagentStatus) -> Option<Self> {
        match status {
            SubagentStatus::Running => None,
            SubagentStatus::Completed => Some(Self::Finished),
            SubagentStatus::Failed => Some(Self::Failed),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Failed)
    }

    /// Verb painted on the chip.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Started => "开始工作",
            Self::Progress => "有进展",
            Self::Finished => "已完成",
            Self::Failed => "运行失败",
        }
    }

    /// Stable, ASCII fragment of the chip's transcript key. Distinct per
    /// phase so a later phase appends a new chip instead of colliding with
    /// an earlier one.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }
}

/// One inline chip. Owned strings (rather than a borrow of the panel's
/// `SubagentSnapshot`) because a chip outlives the snapshot that produced
/// it - the panel keeps only the latest state per agent, the transcript
/// keeps the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentChip {
    /// Registry identity of the sub-agent, matching `SubagentSnapshot::id`.
    pub agent_id: String,
    pub display_name: String,
    pub agent_type: String,
    pub phase: SubagentChipPhase,
    /// One-line preview of the agent's final answer. Only ever set on a
    /// terminal chip, mirroring `SubagentSnapshot::result_summary`.
    pub summary: Option<String>,
    /// Model the sub-agent ran under, for the per-agent disclosure. `None`
    /// when the runtime could not resolve one (older snapshots, or a driver
    /// with no session engine).
    pub model: Option<String>,
}

impl SubagentChip {
    pub fn from_snapshot(snapshot: &SubagentSnapshot, phase: SubagentChipPhase) -> Self {
        Self {
            agent_id: snapshot.id.clone(),
            display_name: snapshot.display_name.clone(),
            agent_type: snapshot.agent_type.clone(),
            phase,
            summary: phase
                .is_terminal()
                .then(|| snapshot.result_summary.clone())
                .flatten(),
            model: snapshot.model.clone(),
        }
    }

    /// Leading text: who did what.
    pub fn headline(&self) -> String {
        format!("{} · {}", self.display_name, self.phase.label())
    }

    /// Per-agent model disclosure, or `None` when the model is unknown.
    pub fn model_label(&self) -> Option<String> {
        self.model.as_deref().map(|model| format!("使用 {model}"))
    }

    /// Trailing text after the headline: what the agent reported when it
    /// finished, falling back to the model disclosure so a chip that has no
    /// summary still says which model did the work.
    pub fn detail(&self) -> Option<String> {
        self.summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
            .map(str::to_owned)
            .or_else(|| self.model_label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_node_protocol::TurnId;

    fn snapshot(status: SubagentStatus, summary: Option<&str>) -> SubagentSnapshot {
        SubagentSnapshot {
            id: "7".into(),
            agent_type: "reviewer".into(),
            display_name: "审查代码".into(),
            depth: 0,
            status,
            tokens: 120,
            turn_id: TurnId::new(),
            completed_at_ms: None,
            result_summary: summary.map(str::to_owned),
            model: Some("claude-opus-5".into()),
        }
    }

    #[test]
    fn a_running_snapshot_has_no_terminal_phase() {
        assert_eq!(
            SubagentChipPhase::from_status(SubagentStatus::Running),
            None
        );
        assert_eq!(
            SubagentChipPhase::from_status(SubagentStatus::Failed),
            Some(SubagentChipPhase::Failed)
        );
    }

    #[test]
    fn only_terminal_chips_carry_the_result_summary() {
        let running = snapshot(SubagentStatus::Running, Some("已读取三个文件"));
        let chip = SubagentChip::from_snapshot(&running, SubagentChipPhase::Progress);
        assert_eq!(chip.summary, None, "a mid-run chip never claims a result");
        assert_eq!(chip.detail().as_deref(), Some("使用 claude-opus-5"));

        let done = snapshot(SubagentStatus::Completed, Some("已读取三个文件"));
        let chip = SubagentChip::from_snapshot(&done, SubagentChipPhase::Finished);
        assert_eq!(chip.detail().as_deref(), Some("已读取三个文件"));
        assert_eq!(chip.headline(), "审查代码 · 已完成");
    }

    #[test]
    fn an_unknown_model_leaves_the_disclosure_off() {
        let mut done = snapshot(SubagentStatus::Completed, None);
        done.model = None;
        let chip = SubagentChip::from_snapshot(&done, SubagentChipPhase::Finished);
        assert_eq!(chip.model_label(), None);
        assert_eq!(chip.detail(), None);
    }
}
