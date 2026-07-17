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
    for thread in &threads {
        state.transcripts.entry(thread.session.clone()).or_default();
    }
    if state
        .current_session
        .as_ref()
        .is_some_and(|session| !known.contains(session))
    {
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
