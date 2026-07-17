use zode_node_protocol::WorkspaceUri;

use super::{
    open_read_no_follow, safe_relative, same_file_identity, workspace_root, ExternalOpenService,
    ServiceError,
};

#[derive(Default)]
pub struct LocalExternalOpenService;

impl ExternalOpenService for LocalExternalOpenService {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError> {
        let root = workspace_root(workspace)?.canonicalize()?;
        let path = root.join(safe_relative(relative)?).canonicalize()?;
        ensure_inside(&root, &path)?;
        let before = std::fs::metadata(&path)?;
        ensure_regular(&path, &before)?;
        let guard = open_read_no_follow(&path)?;
        let opened = guard.metadata()?;
        ensure_regular(&path, &opened)?;
        if !same_file_identity(&before, &opened) {
            return Err(ServiceError::FileChanged(path.display().to_string()));
        }
        let current = path.canonicalize()?;
        ensure_inside(&root, &current)?;
        let current_metadata = std::fs::metadata(&current)?;
        if !same_file_identity(&opened, &current_metadata) {
            return Err(ServiceError::FileChanged(path.display().to_string()));
        }
        let result = open::that(current).map_err(ServiceError::Io);
        drop(guard);
        result
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(ServiceError::InvalidPath(url.to_owned()));
        }
        open::that(url).map_err(ServiceError::Io)
    }
}

fn ensure_inside(root: &std::path::Path, path: &std::path::Path) -> Result<(), ServiceError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(ServiceError::InvalidPath(path.display().to_string()))
    }
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
