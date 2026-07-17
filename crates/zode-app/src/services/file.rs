use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use zode_node_protocol::WorkspaceUri;

use super::{
    open_read_no_follow, safe_relative, same_file_identity, workspace_root, FileMetadata,
    FileService, ServiceError,
};

#[derive(Default)]
pub struct LocalFileService;

#[async_trait]
impl FileService for LocalFileService {
    async fn read(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        let (mut file, _, _) = open_validated_regular(workspace, relative).await?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await?;
        Ok(bytes)
    }

    async fn read_bounded(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ServiceError> {
        let (file, metadata, _) = open_validated_regular(workspace, relative).await?;
        if metadata.len() > max_bytes {
            return Err(ServiceError::FileTooLarge {
                len: metadata.len(),
                max: max_bytes,
            });
        }

        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len().min(max_bytes)).unwrap_or(0));
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > max_bytes {
            return Err(ServiceError::FileTooLarge {
                len: bytes.len() as u64,
                max: max_bytes,
            });
        }
        Ok(bytes)
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
        let (_, path) = resolve_existing(workspace, relative).await?;
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
) -> Result<(std::path::PathBuf, std::path::PathBuf), ServiceError> {
    let root = tokio::fs::canonicalize(workspace_root(workspace)?).await?;
    let path = tokio::fs::canonicalize(root.join(safe_relative(relative)?)).await?;
    ensure_inside(&root, &path)?;
    Ok((root, path))
}

async fn open_validated_regular(
    workspace: &WorkspaceUri,
    relative: &str,
) -> Result<(tokio::fs::File, std::fs::Metadata, std::path::PathBuf), ServiceError> {
    let (root, path) = resolve_existing(workspace, relative).await?;
    let before = tokio::fs::metadata(&path).await?;
    ensure_regular(&path, &before)?;

    let open_path = path.clone();
    let std_file = tokio::task::spawn_blocking(move || open_read_no_follow(&open_path))
        .await
        .map_err(|error| ServiceError::Platform(format!("file open task failed: {error}")))??;
    let file = tokio::fs::File::from_std(std_file);
    let opened = file.metadata().await?;
    ensure_regular(&path, &opened)?;
    if !same_file_identity(&before, &opened) {
        return Err(ServiceError::FileChanged(path.display().to_string()));
    }

    let current_path = tokio::fs::canonicalize(&path).await?;
    ensure_inside(&root, &current_path)?;
    let current = tokio::fs::metadata(&current_path).await?;
    if !same_file_identity(&opened, &current) {
        return Err(ServiceError::FileChanged(path.display().to_string()));
    }
    Ok((file, opened, current_path))
}

fn ensure_regular(
    path: &std::path::Path,
    metadata: &std::fs::Metadata,
) -> Result<(), ServiceError> {
    if metadata.is_dir() {
        Err(ServiceError::IsDirectory(path.display().to_string()))
    } else if !metadata.is_file() {
        Err(ServiceError::UnsupportedFileType(
            path.display().to_string(),
        ))
    } else {
        Ok(())
    }
}

fn ensure_inside(root: &std::path::Path, path: &std::path::Path) -> Result<(), ServiceError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(ServiceError::InvalidPath(path.display().to_string()))
    }
}
