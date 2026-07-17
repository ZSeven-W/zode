use zode_app::services::{FileService, LocalFileService, ServiceError};
use zode_app_runtime::path_to_workspace_uri;

const ONE_MIB: u64 = 1024 * 1024;

struct TempWorkspace {
    path: std::path::PathBuf,
}

impl TempWorkspace {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zode-file-service-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test]
async fn bounded_read_decodes_percent_encoded_non_ascii_workspace_uri() {
    let root = TempWorkspace::new("project with spaces-设计");
    let nested = root.path.join("目录");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("报告.md"), "真实内容").unwrap();
    let workspace = path_to_workspace_uri(&root.path).unwrap();

    let bytes = LocalFileService
        .read_bounded(&workspace, "目录/报告.md", ONE_MIB)
        .await
        .unwrap();

    assert_eq!(bytes, "真实内容".as_bytes());
}

#[tokio::test]
async fn bounded_read_rejects_unsafe_relative_paths_and_nul() {
    let root = TempWorkspace::new("unsafe");
    std::fs::write(root.path.join("safe.md"), "safe").unwrap();
    let workspace = path_to_workspace_uri(&root.path).unwrap();

    for relative in ["", "../outside.md", "/etc/passwd", "a/./b", "nul\0.md"] {
        let error = LocalFileService
            .read_bounded(&workspace, relative, ONE_MIB)
            .await
            .expect_err(relative);
        assert!(matches!(error, ServiceError::InvalidPath(_)), "{error:?}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_read_rejects_symlink_escape_after_canonicalization() {
    let root = TempWorkspace::new("symlink-root");
    let outside = TempWorkspace::new("symlink-outside");
    std::fs::write(outside.path.join("secret.md"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path.join("secret.md"), root.path.join("escape.md"))
        .unwrap();
    let workspace = path_to_workspace_uri(&root.path).unwrap();

    let error = LocalFileService
        .read_bounded(&workspace, "escape.md", ONE_MIB)
        .await
        .expect_err("a symlink outside the workspace must not be read");

    assert!(matches!(error, ServiceError::InvalidPath(_)), "{error:?}");
}

#[tokio::test]
async fn bounded_read_rejects_directory_missing_and_over_limit_files() {
    let root = TempWorkspace::new("limits");
    std::fs::create_dir(root.path.join("folder")).unwrap();
    std::fs::write(
        root.path.join("large.txt"),
        vec![b'x'; ONE_MIB as usize + 1],
    )
    .unwrap();
    let workspace = path_to_workspace_uri(&root.path).unwrap();

    let directory = LocalFileService
        .read_bounded(&workspace, "folder", ONE_MIB)
        .await
        .expect_err("directories are not preview documents");
    assert!(matches!(directory, ServiceError::IsDirectory(_)));

    let missing = LocalFileService
        .read_bounded(&workspace, "missing.md", ONE_MIB)
        .await
        .expect_err("missing files preserve the io error");
    assert!(
        matches!(missing, ServiceError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
    );

    let large = LocalFileService
        .read_bounded(&workspace, "large.txt", ONE_MIB)
        .await
        .expect_err("oversized files are bounded");
    assert!(matches!(
        large,
        ServiceError::FileTooLarge { len, max } if len == ONE_MIB + 1 && max == ONE_MIB
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_read_rejects_fifo_without_waiting_for_a_writer() {
    use std::time::Duration;

    let root = TempWorkspace::new("fifo");
    let fifo = root.path.join("hang.md");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    let workspace = path_to_workspace_uri(&root.path).unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        LocalFileService.read_bounded(&workspace, "hang.md", ONE_MIB),
    )
    .await
    .expect("FIFO validation must not block waiting for a writer")
    .expect_err("FIFO is not a previewable regular file");

    assert!(matches!(result, ServiceError::UnsupportedFileType(_)));
}
