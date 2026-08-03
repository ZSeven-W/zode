//! Reducer arm that turns sub-agent snapshots into inline transcript chips.
//!
//! The producer side (`EventNormalizer::diff_subagents` in zode-app-runtime)
//! already decides *when* a sub-agent is worth reporting: a status change is
//! emitted immediately, token-only churn at most once a second. This module
//! decides *what that looks like in the transcript*, and its whole job is to
//! keep a busy child agent from turning the conversation into a wall of
//! chips:
//!
//! - `Started` is pushed once, the first time an agent is seen.
//! - `Progress` exists at most once per agent at a time. A later progress
//!   report rewrites that same chip (its summary/model can change) instead
//!   of appending another line.
//! - `Finished`/`Failed` is pushed once and thereafter rewritten in place,
//!   so the authoritative pre-`TurnFinished` diff can correct it without
//!   duplicating it.
//! - A snapshot that reports `Running` again after a terminal chip is
//!   ignored: an agent never un-finishes.

use zode_node_protocol::{SubagentSnapshot, SubagentStatus};

use crate::{SubagentChip, SubagentChipPhase, TranscriptItem, TranscriptState};

/// Applies one `SubagentUpdate` snapshot to the transcript's chip line-up.
pub(super) fn apply_subagent_chip(transcript: &mut TranscriptState, snapshot: &SubagentSnapshot) {
    let phase = match SubagentChipPhase::from_status(snapshot.status) {
        Some(terminal) => terminal,
        None if !has_any_chip(transcript, &snapshot.id) => SubagentChipPhase::Started,
        // A still-running agent whose lifecycle already ended in the
        // transcript (a stale snapshot arriving after the terminal diff)
        // must not reopen it.
        None if has_terminal_chip(transcript, &snapshot.id) => return,
        None => SubagentChipPhase::Progress,
    };
    upsert_chip(transcript, SubagentChip::from_snapshot(snapshot, phase));
}

/// Closes out chips for agents that a finished turn just corrected to a
/// terminal status without the runtime having emitted a matching diff. Mirrors
/// `finalize_running_subagents`, which does the same for the panel rows, and
/// only touches agents this transcript already reported on.
pub(super) fn finalize_subagent_chips(
    transcript: &mut TranscriptState,
    subagents: &[SubagentSnapshot],
) {
    for snapshot in subagents {
        if snapshot.status == SubagentStatus::Running {
            continue;
        }
        if !has_any_chip(transcript, &snapshot.id) {
            continue;
        }
        apply_subagent_chip(transcript, snapshot);
    }
}

/// Rewrites the agent's chip for this phase when it already has one,
/// otherwise appends it. Rewriting an unchanged chip is skipped so a
/// no-op update never invalidates measured layout.
fn upsert_chip(transcript: &mut TranscriptState, chip: SubagentChip) {
    match chip_index(transcript, &chip.agent_id, chip.phase) {
        Some(index) => {
            if transcript.items.get(index) != Some(&TranscriptItem::SubagentChip(chip.clone())) {
                let _ = transcript.replace_item(index, TranscriptItem::SubagentChip(chip));
            }
        }
        None => {
            transcript.items.push(TranscriptItem::SubagentChip(chip));
            transcript.touch_layout();
        }
    }
}

fn chip_index(
    transcript: &TranscriptState,
    agent_id: &str,
    phase: SubagentChipPhase,
) -> Option<usize> {
    transcript.items.iter().rposition(
        |item| matches!(item, TranscriptItem::SubagentChip(chip) if chip.agent_id == agent_id && chip.phase == phase),
    )
}

fn has_any_chip(transcript: &TranscriptState, agent_id: &str) -> bool {
    transcript
        .items
        .iter()
        .any(|item| matches!(item, TranscriptItem::SubagentChip(chip) if chip.agent_id == agent_id))
}

fn has_terminal_chip(transcript: &TranscriptState, agent_id: &str) -> bool {
    transcript.items.iter().any(
        |item| matches!(item, TranscriptItem::SubagentChip(chip) if chip.agent_id == agent_id && chip.phase.is_terminal()),
    )
}

#[cfg(test)]
mod tests {
    use zode_node_protocol::TurnId;

    use super::*;

    fn snapshot(id: &str, status: SubagentStatus, tokens: u64) -> SubagentSnapshot {
        SubagentSnapshot {
            id: id.to_owned(),
            agent_type: "reviewer".into(),
            display_name: "审查代码".into(),
            depth: 0,
            status,
            tokens,
            turn_id: TurnId::new(),
            completed_at_ms: None,
            result_summary: None,
            model: Some("claude-opus-5".into()),
        }
    }

    fn phases(transcript: &TranscriptState) -> Vec<SubagentChipPhase> {
        transcript
            .items
            .iter()
            .filter_map(|item| match item {
                TranscriptItem::SubagentChip(chip) => Some(chip.phase),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_first_sighting_starts_and_later_progress_never_stacks() {
        let mut transcript = TranscriptState::default();

        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 10));
        assert_eq!(phases(&transcript), vec![SubagentChipPhase::Started]);

        for tokens in [20, 30, 40] {
            apply_subagent_chip(
                &mut transcript,
                &snapshot("1", SubagentStatus::Running, tokens),
            );
        }
        assert_eq!(
            phases(&transcript),
            vec![SubagentChipPhase::Started, SubagentChipPhase::Progress],
            "repeated progress rewrites one chip instead of appending"
        );
    }

    #[test]
    fn finishing_appends_once_and_is_rewritten_in_place_afterwards() {
        let mut transcript = TranscriptState::default();
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 10));
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 20));

        let mut done = snapshot("1", SubagentStatus::Completed, 30);
        done.result_summary = Some("已读取三个文件".into());
        apply_subagent_chip(&mut transcript, &done);
        apply_subagent_chip(&mut transcript, &done);

        assert_eq!(
            phases(&transcript),
            vec![
                SubagentChipPhase::Started,
                SubagentChipPhase::Progress,
                SubagentChipPhase::Finished
            ]
        );
        let TranscriptItem::SubagentChip(chip) = transcript.items.last().unwrap() else {
            panic!("the last item must be the finish chip");
        };
        assert_eq!(chip.summary.as_deref(), Some("已读取三个文件"));
        assert_eq!(chip.model.as_deref(), Some("claude-opus-5"));
    }

    #[test]
    fn a_stale_running_snapshot_never_reopens_a_finished_agent() {
        let mut transcript = TranscriptState::default();
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 10));
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Failed, 20));
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 30));

        assert_eq!(
            phases(&transcript),
            vec![SubagentChipPhase::Started, SubagentChipPhase::Failed]
        );
    }

    #[test]
    fn two_agents_keep_independent_chip_lines() {
        let mut transcript = TranscriptState::default();
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 10));
        apply_subagent_chip(&mut transcript, &snapshot("2", SubagentStatus::Running, 10));
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 20));
        apply_subagent_chip(&mut transcript, &snapshot("2", SubagentStatus::Running, 20));

        assert_eq!(
            phases(&transcript),
            vec![
                SubagentChipPhase::Started,
                SubagentChipPhase::Started,
                SubagentChipPhase::Progress,
                SubagentChipPhase::Progress
            ]
        );
    }

    #[test]
    fn finalizing_only_closes_agents_the_transcript_already_reported() {
        let mut transcript = TranscriptState::default();
        apply_subagent_chip(&mut transcript, &snapshot("1", SubagentStatus::Running, 10));

        finalize_subagent_chips(
            &mut transcript,
            &[
                snapshot("1", SubagentStatus::Failed, 10),
                snapshot("2", SubagentStatus::Completed, 10),
                snapshot("3", SubagentStatus::Running, 10),
            ],
        );

        assert_eq!(
            phases(&transcript),
            vec![SubagentChipPhase::Started, SubagentChipPhase::Failed],
            "an agent with no chip of its own is not introduced at the end of a turn"
        );
    }
}
