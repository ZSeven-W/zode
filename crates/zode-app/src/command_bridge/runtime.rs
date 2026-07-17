use zode_app_model::{AppCommand, ZodeAppState};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentQuery, AgentSnapshot, RuntimeOptions,
    SessionLocator, PROTOCOL_VERSION,
};

use super::{current_session, CommandDispatch, Completion};

pub(super) async fn query_session_runtime_options(
    endpoint: &dyn AgentEndpoint,
    session: &SessionLocator,
) -> Result<RuntimeOptions, String> {
    match endpoint
        .query(AgentQuery::SessionRuntimeOptions {
            session: session.clone(),
        })
        .await
        .map_err(|error| error.to_string())?
    {
        AgentSnapshot::SessionRuntimeOptions {
            session: snapshot_session,
            options,
        } if &snapshot_session == session => Ok(options),
        AgentSnapshot::SessionRuntimeOptions { .. } => {
            Err("the endpoint returned runtime options for the wrong session".into())
        }
        _ => Err("the endpoint returned the wrong runtime-options snapshot".into()),
    }
}

pub(super) fn ensure_runtime_idle(
    state: &ZodeAppState,
    session: &SessionLocator,
) -> Result<(), String> {
    if state.active_turns.contains_key(session) {
        Err("runtime options cannot change while the task is running".into())
    } else {
        Ok(())
    }
}

pub(super) fn prepare_permission_preset(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> Result<CommandDispatch, String> {
    let AppCommand::SetPermissionPreset {
        approval_mode,
        sandbox_mode,
        network,
    } = command
    else {
        unreachable!("permission preset preparation received another command")
    };
    let session = current_session(state)?;
    ensure_runtime_idle(state, &session)?;
    state.composer.footer_menu = None;
    Ok(CommandDispatch {
        commands: vec![AgentCommand {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id: None,
            kind: AgentCommandKind::SetPermissionPreset {
                approval_mode,
                sandbox_mode,
                network,
            },
        }],
        completion: Completion::RefreshRuntimeOptions { session },
    })
}

pub(super) fn prepare_reset_runtime(state: &mut ZodeAppState) -> Result<CommandDispatch, String> {
    let session = current_session(state)?;
    ensure_runtime_idle(state, &session)?;
    let defaults = state
        .composer_defaults
        .clone()
        .ok_or_else(|| "the default runtime options are unavailable".to_owned())?;
    let mut commands = Vec::with_capacity(3);
    if let Some(model) = defaults.active_model {
        commands.push(AgentCommand {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id: None,
            kind: AgentCommandKind::SetModel { model },
        });
    }
    if let Some(effort) = defaults.effort {
        commands.push(AgentCommand {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id: None,
            kind: AgentCommandKind::SetEffort { effort },
        });
    }
    commands.push(AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id: None,
        kind: AgentCommandKind::SetPermissionPreset {
            approval_mode: defaults.approval_mode,
            sandbox_mode: defaults.sandbox_mode,
            network: defaults.sandbox_network,
        },
    });
    state.composer.footer_menu = None;
    Ok(CommandDispatch {
        commands,
        completion: Completion::RefreshRuntimeOptions { session },
    })
}
