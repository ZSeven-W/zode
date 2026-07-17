use async_trait::async_trait;
use zode_node_protocol::WorkspaceUri;

use super::{ServiceError, WorkspaceService};

#[derive(Default)]
pub struct LocalWorkspaceService;

#[async_trait]
impl WorkspaceService for LocalWorkspaceService {
    async fn pick_workspace(&self) -> Result<Option<WorkspaceUri>, ServiceError> {
        let Some(handle) = rfd::AsyncFileDialog::new().pick_folder().await else {
            return Ok(None);
        };
        let path = handle.path().canonicalize()?;
        zode_app_runtime::path_to_workspace_uri(&path)
            .map(Some)
            .map_err(|error| ServiceError::Platform(error.to_string()))
    }

    async fn recent_workspaces(&self) -> Result<Vec<WorkspaceUri>, ServiceError> {
        Ok(Vec::new())
    }
}
