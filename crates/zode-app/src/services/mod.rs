mod external_open;
mod file;
mod notification;
mod terminal;
mod window;
mod workspace;

use async_trait::async_trait;
use zode_app_model::SystemTheme;
use zode_node_protocol::WorkspaceUri;

pub use external_open::LocalExternalOpenService;
pub use file::LocalFileService;
pub use notification::LocalNotificationService;
pub use terminal::{LocalTerminalService, TerminalError, TerminalOutputStream, TerminalService};
pub use window::NativeWindowService;
pub use workspace::LocalWorkspaceService;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("platform service: {0}")]
    Platform(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub is_dir: bool,
    pub len: u64,
    pub modified_ms: Option<i64>,
}

pub trait WindowService: Send + Sync {
    fn begin_drag(&self) -> Result<(), ServiceError>;
    fn minimize(&self);
    fn toggle_maximize(&self);
    fn close(&self);
    fn toggle_fullscreen(&self);
    fn system_theme(&self) -> SystemTheme;
}

#[async_trait]
pub trait WorkspaceService: Send + Sync {
    async fn pick_workspace(&self) -> Result<Option<WorkspaceUri>, ServiceError>;
    async fn recent_workspaces(&self) -> Result<Vec<WorkspaceUri>, ServiceError>;
}

#[async_trait]
pub trait FileService: Send + Sync {
    async fn read(&self, workspace: &WorkspaceUri, relative: &str)
        -> Result<Vec<u8>, ServiceError>;
    async fn write(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ServiceError>;
    async fn metadata(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
    ) -> Result<FileMetadata, ServiceError>;
}

pub trait NotificationService: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<(), ServiceError>;
}

pub trait ExternalOpenService: Send + Sync {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError>;
    fn open_url(&self, url: &str) -> Result<(), ServiceError>;
}

pub(crate) fn workspace_root(workspace: &WorkspaceUri) -> Result<std::path::PathBuf, ServiceError> {
    let value = workspace.as_str();
    let path = value.strip_prefix("file://").ok_or_else(|| {
        ServiceError::CapabilityDenied(format!("workspace is not local: {value}"))
    })?;
    Ok(std::path::PathBuf::from(path))
}

pub(crate) fn safe_relative(relative: &str) -> Result<std::path::PathBuf, ServiceError> {
    let path = std::path::Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(ServiceError::InvalidPath(relative.to_owned()));
    }
    Ok(path.to_path_buf())
}
