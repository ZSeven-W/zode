use zode_app_model::{TranscriptState, ZodeAppState};
use zode_app_runtime::{path_to_workspace_uri, workspace_uri_to_path};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
    UserContent, WorkspaceUri, PROTOCOL_VERSION,
};

use super::{append_user_content, CommandDispatch, Completion};
use crate::command_projection::now_ms;

pub(super) fn prepare_first_submit(
    state: &mut ZodeAppState,
    input: Vec<UserContent>,
) -> Result<CommandDispatch, String> {
    let session = SessionLocator::new(state.host.node_id, uuid::Uuid::new_v4().to_string());
    let workspace_uri = first_session_workspace(state, &session)?;
    let turn_id = TurnId::new();
    let create = AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id: None,
        kind: AgentCommandKind::CreateSession {
            workspace_uri: workspace_uri.clone(),
            model: state.composer.model.clone(),
        },
    };
    let start = AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id: Some(turn_id),
        kind: AgentCommandKind::StartTurn {
            input: input.clone(),
        },
    };
    let mut transcript = TranscriptState::default();
    let start_item_index = transcript.items.len();
    append_user_content(&mut transcript, &input);
    let response_item_index = transcript.items.len();
    let started = transcript.begin_turn(turn_id, start_item_index, response_item_index);
    debug_assert!(started, "a fresh transcript accepts its first turn");
    state.threads.insert(
        0,
        ThreadSummary {
            session: session.clone(),
            workspace_uri,
            title: "新任务".into(),
            updated_at_ms: now_ms(),
            status: ThreadStatus::Running,
        },
    );
    state.transcripts.insert(session.clone(), transcript);
    state.active_turns.insert(session.clone(), turn_id);
    state.current_session = Some(session.clone());
    Ok(CommandDispatch {
        commands: vec![create, start],
        completion: Completion::RefreshRuntimeOptions { session },
    })
}

fn first_session_workspace(
    state: &ZodeAppState,
    session: &SessionLocator,
) -> Result<WorkspaceUri, String> {
    if let Some(workspace_uri) = state.active_workspace.as_ref() {
        return state
            .available_workspace(workspace_uri)
            .then(|| workspace_uri.clone())
            .ok_or_else(|| "the selected workspace is unavailable for a new session".to_owned());
    }

    let root_uri = state
        .projectless_workspace_root
        .as_ref()
        .ok_or_else(|| "the projectless task workspace is unavailable".to_owned())?;
    let root = workspace_uri_to_path(root_uri).map_err(|error| error.to_string())?;
    let workspace = root.join(&session.session_id);
    create_private_dir(&workspace).map_err(|error| {
        format!(
            "failed to create the projectless task workspace {}: {error}",
            workspace.display()
        )
    })?;
    path_to_workspace_uri(&workspace).map_err(|error| error.to_string())
}

fn create_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
