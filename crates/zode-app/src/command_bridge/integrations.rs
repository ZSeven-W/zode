use zode_app_model::{
    integration_catalog, Availability, IntegrationCategory, IntegrationInstallState,
    IntegrationMutationState, LoadState, ZodeAppState,
};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentQuery, AgentSnapshot, SessionLocator,
    WorkspaceUri,
};

use super::{Completion, CompletionResult};

pub(super) struct IntegrationDispatchParts {
    pub session: SessionLocator,
    pub kind: AgentCommandKind,
    pub completion: Completion,
}

pub(super) fn prepare(
    state: &mut ZodeAppState,
    workspace_uri: WorkspaceUri,
    source_id: String,
    enabled: bool,
) -> Result<IntegrationDispatchParts, String> {
    let catalog = state
        .presentation
        .integrations
        .ready()
        .filter(|catalog| catalog.workspace_uri == workspace_uri)
        .ok_or_else(|| "the integration catalog is unavailable for this workspace".to_owned())?;
    let entry = catalog
        .all_entries()
        .find(|entry| entry.source_id.as_deref() == Some(source_id.as_str()))
        .ok_or_else(|| "the integration is no longer present in the local registry".to_owned())?;
    if entry.category == IntegrationCategory::Capabilities {
        return Err("node capabilities cannot be changed from the plugin page".into());
    }
    if entry.install_state == IntegrationInstallState::Available {
        return Err("this node has no verified integration installer".into());
    }
    let currently_enabled = entry.availability != Availability::Disabled;
    if currently_enabled == enabled {
        return Err("the integration already has the requested state".into());
    }
    let session = command_session(state, &workspace_uri)?;
    state.presentation.integration_mutation = IntegrationMutationState::Updating {
        source_id: source_id.clone(),
        enabled,
    };
    Ok(IntegrationDispatchParts {
        session,
        kind: AgentCommandKind::SetIntegrationEnabled {
            workspace_uri: workspace_uri.clone(),
            source_id,
            enabled,
        },
        completion: Completion::RefreshIntegrations { workspace_uri },
    })
}

fn command_session(
    state: &ZodeAppState,
    workspace_uri: &WorkspaceUri,
) -> Result<SessionLocator, String> {
    if let Some(session) = state
        .current_session
        .as_ref()
        .filter(|session| state.available_workspace_for_session(session) == Some(workspace_uri))
    {
        if state.active_turns.contains_key(session)
            || state
                .transcripts
                .get(session)
                .is_some_and(|transcript| transcript.busy)
        {
            return Err("stop the active task before changing integrations".into());
        }
        return Ok(session.clone());
    }
    if let Some(session) = state
        .threads
        .iter()
        .filter(|thread| &thread.workspace_uri == workspace_uri)
        .map(|thread| &thread.session)
        .find(|session| {
            !state.active_turns.contains_key(*session)
                && !state
                    .transcripts
                    .get(*session)
                    .is_some_and(|transcript| transcript.busy)
        })
    {
        return Ok(session.clone());
    }
    Ok(SessionLocator::new(
        state.host.node_id,
        "integration-settings",
    ))
}

pub(super) async fn complete(
    endpoint: &dyn AgentEndpoint,
    workspace_uri: WorkspaceUri,
) -> Result<CompletionResult, String> {
    match endpoint
        .query(AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        })
        .await
        .map_err(|error| error.to_string())?
    {
        AgentSnapshot::Integrations(snapshot) if snapshot.workspace_uri == workspace_uri => {
            Ok(CompletionResult::Integrations(snapshot))
        }
        AgentSnapshot::Integrations(_) => {
            Err("the endpoint returned integrations for the wrong workspace".into())
        }
        _ => Err("the endpoint returned the wrong integrations snapshot".into()),
    }
}

pub(super) fn apply_success(
    state: &mut ZodeAppState,
    snapshot: zode_node_protocol::IntegrationRegistrySnapshot,
) {
    state.presentation.integrations = LoadState::Ready(integration_catalog(snapshot));
    state.presentation.integration_mutation = IntegrationMutationState::Idle;
}

pub(super) fn mark_failure(state: &mut ZodeAppState, command: &AgentCommand, message: &str) {
    if let AgentCommandKind::SetIntegrationEnabled { source_id, .. } = &command.kind {
        state.presentation.integration_mutation = IntegrationMutationState::Failed {
            source_id: source_id.clone(),
            message: message.to_owned(),
        };
    }
}
