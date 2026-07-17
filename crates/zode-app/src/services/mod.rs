#[path = "external-application.rs"]
mod external_application;
mod external_open;
mod file;
mod notification;
mod repository;
#[path = "session-window.rs"]
mod session_window;
mod terminal;
mod window;
mod workspace;

use async_trait::async_trait;
use zode_app_model::SystemTheme;
use zode_node_protocol::{EndpointErrorKind, WorkspaceUri};

pub use external_application::LocalExternalApplicationService;
pub use external_open::LocalExternalOpenService;
pub use file::LocalFileService;
pub use notification::LocalNotificationService;
pub use repository::LocalRepositoryService;
pub use session_window::{NativeSessionWindowService, SessionWindowService};
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
    #[error("is a directory: {0}")]
    IsDirectory(String),
    #[error("not a directory: {0}")]
    NotDirectory(String),
    #[error("unsupported file type: {0}")]
    UnsupportedFileType(String),
    #[error("file changed while it was being opened: {0}")]
    FileChanged(String),
    #[error("file is too large: {len} bytes exceeds the {max} byte limit")]
    FileTooLarge { len: u64, max: u64 },
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
    async fn read_bounded(
        &self,
        workspace: &WorkspaceUri,
        relative: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ServiceError>;
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

pub trait ExternalApplicationService: Send + Sync {
    fn installed_applications(
        &self,
    ) -> Result<zode_app_model::ExternalApplicationCatalog, ServiceError>;
    fn open_workspace(
        &self,
        workspace: &WorkspaceUri,
        application: zode_app_model::ExternalApplication,
    ) -> Result<(), ServiceError>;
}

pub trait ExternalOpenService: Send + Sync {
    fn open_file(&self, workspace: &WorkspaceUri, relative: &str) -> Result<(), ServiceError>;
    fn open_url(&self, url: &str) -> Result<(), ServiceError>;
    /// Opens the named System Settings privacy pane. A dedicated method
    /// (rather than a caller-supplied string through `open_url`, whose
    /// contract is `http(s)://` only) because the `x-apple.systempreferences:`
    /// scheme is only ever launched with these two known-good, hardcoded
    /// URLs - never with agent- or user-supplied text.
    fn open_system_settings_pane(
        &self,
        pane: zode_app_model::ComputerPermissionKind,
    ) -> Result<(), ServiceError>;
}

/// Safe local repository-adjacent platform actions.
///
/// Git mutations are intentionally absent: the desktop cannot commit or push
/// until a separately reviewed service contract exists.
pub trait RepositoryService: Send + Sync {
    fn open_workspace(&self, workspace: &WorkspaceUri) -> Result<(), ServiceError>;
}

pub(crate) fn workspace_root(workspace: &WorkspaceUri) -> Result<std::path::PathBuf, ServiceError> {
    zode_app_runtime::workspace_uri_to_path(workspace).map_err(|error| match error.kind {
        EndpointErrorKind::CapabilityDenied => ServiceError::CapabilityDenied(error.message),
        _ => ServiceError::InvalidPath(error.message),
    })
}

pub(crate) fn safe_relative(relative: &str) -> Result<std::path::PathBuf, ServiceError> {
    let path = std::path::Path::new(relative);
    let bytes = relative.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let forbidden_segment = relative
        .split(['/', '\\'])
        .any(|segment| matches!(segment, "." | ".."));
    if relative.is_empty()
        || relative.contains('\0')
        || windows_absolute
        || forbidden_segment
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(ServiceError::InvalidPath(relative.to_owned()));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn open_read_no_follow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

pub(crate) fn file_identity(file: &std::fs::File) -> std::io::Result<same_file::Handle> {
    same_file::Handle::from_file(file.try_clone()?)
}

#[cfg(test)]
mod tests {
    use super::file_identity;

    #[test]
    fn file_identity_distinguishes_replaced_objects() {
        let root =
            std::env::temp_dir().join(format!("zode-file-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        std::fs::write(&first, b"same length").unwrap();
        std::fs::write(&second, b"same length").unwrap();
        let first_file = std::fs::File::open(&first).unwrap();
        let first_again = std::fs::File::open(&first).unwrap();
        let second_file = std::fs::File::open(&second).unwrap();

        assert_eq!(
            file_identity(&first_file).unwrap(),
            file_identity(&first_again).unwrap()
        );
        assert_ne!(
            file_identity(&first_file).unwrap(),
            file_identity(&second_file).unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
