use std::time::SystemTime;

use zode_app_model::{TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

pub(super) fn project_global_error(state: &mut ZodeAppState, message: String) {
    let workspace_uri = state
        .projects
        .first()
        .map(|project| project.workspace_uri.clone())
        .unwrap_or_else(|| WorkspaceUri::new("file:///").expect("root file URI is valid"));
    let session = SessionLocator::new(
        state.host.node_id,
        format!("local-error-{}", uuid::Uuid::new_v4()),
    );
    state.threads.insert(
        0,
        ThreadSummary {
            session: session.clone(),
            workspace_uri,
            title: "命令错误".into(),
            updated_at_ms: now_ms(),
            status: ThreadStatus::Failed,
        },
    );
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![TranscriptItem::Error {
                message: format!("命令执行失败：{message}"),
                retryable: true,
            }],
            ..TranscriptState::default()
        },
    );
    state.current_session = Some(session);
}

pub(super) fn replace_threads(state: &mut ZodeAppState, threads: Vec<ThreadSummary>) {
    let known = threads
        .iter()
        .map(|thread| thread.session.clone())
        .collect::<std::collections::BTreeSet<_>>();
    state
        .transcripts
        .retain(|session, _| known.contains(session));
    state
        .message_queues
        .retain(|session, _| known.contains(session));
    state
        .active_turns
        .retain(|session, _| known.contains(session));
    state
        .tool_expanded
        .retain(|session, _| known.contains(session));
    state.usage.retain(|session, _| known.contains(session));
    state
        .presentation
        .sessions
        .retain(|session, _| known.contains(session));
    state.approvals.retain(|_, session| known.contains(session));
    if state
        .pending_session_delete
        .as_ref()
        .is_some_and(|session| !known.contains(session))
    {
        state.pending_session_delete = None;
    }
    for thread in &threads {
        state.transcripts.entry(thread.session.clone()).or_default();
    }
    let current_session_removed = state
        .current_session
        .as_ref()
        .is_some_and(|session| !known.contains(session));
    if current_session_removed {
        state.composer.queue_menu = None;
        state.composer.finish_queue_edit();
        state.current_session = threads
            .iter()
            .filter(|thread| state.available_workspace(&thread.workspace_uri))
            .max_by_key(|thread| thread.updated_at_ms)
            .map(|thread| thread.session.clone());
        if let Some(workspace_uri) = state.current_session.as_ref().and_then(|session| {
            threads
                .iter()
                .find(|thread| &thread.session == session)
                .map(|thread| thread.workspace_uri.clone())
        }) {
            state.active_workspace = Some(workspace_uri);
        }
    }
    state.threads = threads;
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zode_app_model::{demo_state, MessageQueueState, ProjectState, SessionPresentationState};
    use zode_node_protocol::{
        SessionLocator, ThreadStatus, ThreadSummary, TurnId, UsageSnapshot, WorkspaceUri,
    };

    use super::replace_threads;

    #[test]
    fn thread_refresh_removes_every_session_scoped_projection_for_missing_threads() {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        let kept = SessionLocator::new(state.host.node_id, "kept");
        let stale = SessionLocator::new(state.host.node_id, "stale");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.current_session = Some(stale.clone());
        state.pending_session_delete = Some(stale.clone());
        state.transcripts.insert(kept.clone(), Default::default());
        state.transcripts.insert(stale.clone(), Default::default());

        let mut kept_queue = MessageQueueState::default();
        kept_queue.enqueue("kept".into(), Vec::new()).unwrap();
        let mut stale_queue = MessageQueueState::default();
        let stale_message = stale_queue.enqueue("stale".into(), Vec::new()).unwrap();
        state.message_queues.insert(kept.clone(), kept_queue);
        state.message_queues.insert(stale.clone(), stale_queue);
        state.composer.draft = "original draft".into();
        state.composer.begin_queue_edit(stale_message, "stale");

        state.active_turns.insert(kept.clone(), TurnId::new());
        state.active_turns.insert(stale.clone(), TurnId::new());
        state.tool_expanded.insert(kept.clone(), BTreeMap::new());
        state.tool_expanded.insert(stale.clone(), BTreeMap::new());
        let usage = UsageSnapshot {
            input_tokens: 1,
            output_tokens: 2,
            context_used: Some(0.1),
            cost_usd: Some(0.01),
        };
        state.usage.insert(kept.clone(), usage.clone());
        state.usage.insert(stale.clone(), usage);
        state
            .presentation
            .sessions
            .insert(kept.clone(), SessionPresentationState::default());
        state
            .presentation
            .sessions
            .insert(stale.clone(), SessionPresentationState::default());
        state.approvals.insert("kept-approval".into(), kept.clone());
        state
            .approvals
            .insert("stale-approval".into(), stale.clone());

        replace_threads(
            &mut state,
            vec![ThreadSummary {
                session: kept.clone(),
                workspace_uri: workspace,
                title: "kept".into(),
                updated_at_ms: 1,
                status: ThreadStatus::Idle,
            }],
        );

        assert_eq!(state.current_session.as_ref(), Some(&kept));
        assert_eq!(state.pending_session_delete, None);
        assert_eq!(state.composer.editing_queued_message, None);
        assert_eq!(state.composer.draft, "original draft");
        assert!(state.transcripts.contains_key(&kept));
        assert!(state.message_queues.contains_key(&kept));
        assert!(state.active_turns.contains_key(&kept));
        assert!(state.tool_expanded.contains_key(&kept));
        assert!(state.usage.contains_key(&kept));
        assert!(state.presentation.sessions.contains_key(&kept));
        assert_eq!(state.approvals.get("kept-approval"), Some(&kept));

        assert!(!state.transcripts.contains_key(&stale));
        assert!(!state.message_queues.contains_key(&stale));
        assert!(!state.active_turns.contains_key(&stale));
        assert!(!state.tool_expanded.contains_key(&stale));
        assert!(!state.usage.contains_key(&stale));
        assert!(!state.presentation.sessions.contains_key(&stale));
        assert!(!state.approvals.contains_key("stale-approval"));
    }
}
