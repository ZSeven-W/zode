//! One-line warning when the user changes model mid-conversation.
//!
//! A model swap is free on a fresh task and lossy on an ongoing one: the
//! incoming model starts from the compacted context rather than everything
//! the previous model actually read, so answer quality can drop. Codex warns
//! about exactly this; the notice below is the same warning in the house
//! transcript-notice style (`TranscriptItem::Status`, like the approval
//! notices next door).
//!
//! Emitted at most once per switch, and never for a task that has not said
//! anything yet - a model picked before the first message is a preference,
//! not a switch.

use zode_app_model::{TranscriptItem, ZodeAppState};
use zode_node_protocol::SessionLocator;

pub(super) const MODEL_SWITCH_NOTICE_CODE: &str = "session.model_switched";

pub(super) fn note_model_switch(state: &mut ZodeAppState, session: &SessionLocator, model: &str) {
    let previous = state.composer.model.as_deref().unwrap_or_default();
    if previous.is_empty() || previous == model {
        return;
    }
    let Some(transcript) = state.transcripts.get_mut(session) else {
        return;
    };
    if !transcript.items.iter().any(|item| {
        matches!(
            item,
            TranscriptItem::UserText { .. } | TranscriptItem::AssistantText { .. }
        )
    }) {
        return;
    }
    let notice = TranscriptItem::Status {
        code: MODEL_SWITCH_NOTICE_CODE.to_owned(),
        message: format!("已切换到 {model}：新模型需要重新读取已缩减的上下文，可能影响回复效果。"),
    };
    // Flipping between models several times in a row rewrites the same line
    // instead of stacking one warning per flip - the advice does not get
    // truer by repetition.
    match transcript.items.last() {
        Some(TranscriptItem::Status { code, .. }) if code == MODEL_SWITCH_NOTICE_CODE => {
            let last = transcript.items.len() - 1;
            let _ = transcript.replace_item(last, notice);
        }
        _ => {
            transcript.items.push(notice);
            transcript.touch_layout();
        }
    }
}
