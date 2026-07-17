use zode_app_model::{TranscriptItem, ZodeAppState};
use zode_node_protocol::{ApprovalDecision, SessionLocator};

pub(super) fn scoped_decision(
    state: &mut ZodeAppState,
    session: &SessionLocator,
    decision: ApprovalDecision,
) -> ApprovalDecision {
    if decision != ApprovalDecision::AllowAlways {
        return decision;
    }
    let projectless = state
        .threads
        .iter()
        .find(|thread| &thread.session == session)
        .is_some_and(|thread| state.is_projectless_workspace(&thread.workspace_uri));
    if !projectless {
        return decision;
    }
    if let Some(transcript) = state.transcripts.get_mut(session) {
        transcript.items.push(TranscriptItem::Status {
            code: "approval.projectless_allow_once".into(),
            message: "任务模式不会保存项目级权限；本次仅允许一次。".into(),
        });
    }
    ApprovalDecision::AllowOnce
}
