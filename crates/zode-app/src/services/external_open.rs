use zode_node_protocol::WorkspaceUri;

use super::{
    file_identity, open_read_no_follow, safe_relative, workspace_root, ExternalOpenService,
    ServiceError,
};

#[derive(Default)]
pub struct LocalExternalOpenService;

impl ExternalOpenService for LocalExternalOpenService {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError> {
        let root = workspace_root(workspace)?.canonicalize()?;
        let path = root.join(safe_relative(relative)?).canonicalize()?;
        ensure_inside(&root, &path)?;
        let guard = open_read_no_follow(&path)?;
        let opened_identity = file_identity(&guard)?;
        let opened = guard.metadata()?;
        ensure_regular(&path, &opened)?;
        let current = path.canonicalize()?;
        ensure_inside(&root, &current)?;
        let current_guard = open_read_no_follow(&current)?;
        let current_metadata = current_guard.metadata()?;
        ensure_regular(&current, &current_metadata)?;
        if opened_identity != file_identity(&current_guard)? {
            return Err(ServiceError::FileChanged(path.display().to_string()));
        }
        let result = open::that(current).map_err(ServiceError::Io);
        drop(current_guard);
        drop(guard);
        result
    }

    fn open_url(&self, url: &str) -> Result<(), ServiceError> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(ServiceError::InvalidPath(url.to_owned()));
        }
        open::that(url).map_err(ServiceError::Io)
    }

    fn open_system_settings_pane(
        &self,
        pane: zode_app_model::ComputerPermissionKind,
    ) -> Result<(), ServiceError> {
        let url = match pane {
            zode_app_model::ComputerPermissionKind::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            zode_app_model::ComputerPermissionKind::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        };
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
