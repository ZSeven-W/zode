use std::path::PathBuf;

use zode_node_protocol::WorkspaceUri;

use super::{workspace_root, RepositoryService, ServiceError};

#[derive(Default)]
pub struct LocalRepositoryService;

impl RepositoryService for LocalRepositoryService {
    fn open_workspace(&self, workspace: &WorkspaceUri) -> Result<(), ServiceError> {
        open::that(canonical_workspace_directory(workspace)?).map_err(ServiceError::Io)
    }
}

fn canonical_workspace_directory(workspace: &WorkspaceUri) -> Result<PathBuf, ServiceError> {
    let path = workspace_root(workspace)?.canonicalize()?;
    if !path.metadata()?.is_dir() {
        return Err(ServiceError::NotDirectory(path.display().to_string()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use zode_node_protocol::WorkspaceUri;

    use super::canonical_workspace_directory;
    use crate::services::ServiceError;

    #[test]
    fn validates_a_real_local_workspace_without_opening_it() {
        let root =
            std::env::temp_dir().join(format!("zode-open-workspace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = WorkspaceUri::new(format!("file://{}", root.display())).unwrap();

        assert_eq!(
            canonical_workspace_directory(&workspace).unwrap(),
            root.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_remote_workspaces_and_regular_files() {
        let remote = WorkspaceUri::new("zode-node://mac/repo").unwrap();
        assert!(matches!(
            canonical_workspace_directory(&remote),
            Err(ServiceError::CapabilityDenied(_)) | Err(ServiceError::InvalidPath(_))
        ));

        let root =
            std::env::temp_dir().join(format!("zode-open-workspace-{}", uuid::Uuid::new_v4()));
        std::fs::write(&root, b"not a directory").unwrap();
        let workspace = WorkspaceUri::new(format!("file://{}", root.display())).unwrap();
        assert!(matches!(
            canonical_workspace_directory(&workspace),
            Err(ServiceError::NotDirectory(_))
        ));
        let _ = std::fs::remove_file(root);
    }
}
