use zode_node_protocol::WorkspaceUri;

use super::{safe_relative, workspace_root, ExternalOpenService, ServiceError};

#[derive(Default)]
pub struct LocalExternalOpenService;

impl ExternalOpenService for LocalExternalOpenService {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError> {
        let root = workspace_root(workspace)?.canonicalize()?;
        let path = root.join(safe_relative(relative)?).canonicalize()?;
        if !path.starts_with(&root) {
            return Err(ServiceError::InvalidPath(path.display().to_string()));
        }
        open::that(path).map_err(ServiceError::Io)
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(ServiceError::InvalidPath(url.to_owned()));
        }
        open::that(url).map_err(ServiceError::Io)
    }
}
