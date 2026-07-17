use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use zode_node_protocol::WorkspaceUri;

use super::{safe_relative, workspace_root, FileMetadata, FileService, ServiceError};

#[derive(Default)]
pub struct LocalFileService;

#[async_trait]
impl FileService for LocalFileService {
    async fn read(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        let path = resolve_existing(workspace, relative).await?;
        Ok(tokio::fs::read(path).await?)
    }

    async fn write(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ServiceError> {
        let root = tokio::fs::canonicalize(workspace_root(workspace)?).await?;
        let relative = safe_relative(relative)?;
        let path = root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| ServiceError::InvalidPath(path.display().to_string()))?;
        let parent = tokio::fs::canonicalize(parent).await?;
        ensure_inside(&root, &parent)?;
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    async fn metadata(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
    ) -> Result<FileMetadata, ServiceError> {
        let path = resolve_existing(workspace, relative).await?;
        let metadata = tokio::fs::metadata(path).await?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .and_then(|value| i64::try_from(value.as_millis()).ok());
        Ok(FileMetadata {
            is_dir: metadata.is_dir(),
            len: metadata.len(),
            modified_ms,
        })
    }
}

async fn resolve_existing(
    workspace: &WorkspaceUri,
    relative: &str,
) -> Result<std::path::PathBuf, ServiceError> {
    let root = tokio::fs::canonicalize(workspace_root(workspace)?).await?;
    let path = tokio::fs::canonicalize(root.join(safe_relative(relative)?)).await?;
    ensure_inside(&root, &path)?;
    Ok(path)
}

fn ensure_inside(root: &std::path::Path, path: &std::path::Path) -> Result<(), ServiceError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(ServiceError::InvalidPath(path.display().to_string()))
    }
}
