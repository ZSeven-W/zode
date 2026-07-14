use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_NO_MORE_FILES, HANDLE,
};
use windows::Win32::Security::{
    DuplicateTokenEx, RevertToSelf, SecurityImpersonation, TokenImpersonation, TOKEN_ALL_ACCESS,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, DeleteFileW, FindClose, FindFirstFileW, FindNextFileW,
    MoveFileExW, RemoveDirectoryW, WriteFile, CREATE_ALWAYS, CREATE_NEW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    WIN32_FIND_DATAW,
};
use windows::Win32::System::Threading::SetThreadToken;

use super::acl::AppliedAcls;
use super::token::RestrictedToken;
use super::SandboxConfig;

#[derive(Debug)]
pub(crate) enum FsOperation {
    Write {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    CreateDir {
        path: PathBuf,
        recursive: bool,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Remove {
        path: PathBuf,
        recursive: bool,
        is_dir: bool,
    },
}

pub(super) async fn run(config: &SandboxConfig, operation: FsOperation) -> io::Result<()> {
    let roots = config.windows_writable_roots();
    let read_only = config.mode == super::SandboxMode::ReadOnly;
    tokio::task::spawn_blocking(move || run_sync(&roots, read_only, || perform(operation)))
        .await
        .map_err(|error| io::Error::other(format!("sandbox fs-op worker failed: {error}")))?
        .map_err(Into::into)
}

pub(super) async fn verify_write_denied(
    config: &SandboxConfig,
    path: PathBuf,
) -> Result<(), String> {
    let roots = config.windows_writable_roots();
    let read_only = config.mode == super::SandboxMode::ReadOnly;
    let probe_path = path.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        run_sync(&roots, read_only, || probe_create(&probe_path))
    })
    .await
    .map_err(|error| format!("Windows sandbox canary worker failed: {error}"))?;
    match outcome {
        Ok(ProbeOutcome::Denied) => Ok(()),
        Ok(ProbeOutcome::Created) => {
            let _ = std::fs::remove_file(&path);
            Err(format!(
                "Windows Tier 1 sandbox is ineffective: a direct CreateFileW probe wrote outside configured roots at {}",
                path.display()
            ))
        }
        Err(error) => Err(format!(
            "Windows sandbox canary could not run at {}: {error}",
            path.display()
        )),
    }
}

fn run_sync<T>(
    roots: &[PathBuf],
    read_only: bool,
    operation: impl FnOnce() -> Result<T, WinError>,
) -> Result<T, WinError> {
    // ACL application is process-global and journals by process id. Serialize
    // it with impersonation so concurrent async fs tools cannot revoke another
    // operation's capability ACE or overwrite its cleanup journal.
    let _lock = operation_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _acls = AppliedAcls::apply(roots).map_err(WinError::setup)?;
    let token = RestrictedToken::create(roots, read_only).map_err(WinError::setup)?;
    let mut impersonation = ImpersonationGuard::begin(&token)?;
    let result = operation();
    let revert = impersonation.revert();
    match (result, revert) {
        (_, Err(error)) => Err(error),
        (result, Ok(())) => result,
    }
}

fn operation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct ImpersonationGuard {
    token: HANDLE,
    active: bool,
}

impl ImpersonationGuard {
    fn begin(token: &RestrictedToken) -> Result<Self, WinError> {
        unsafe {
            let mut impersonation = HANDLE::default();
            DuplicateTokenEx(
                token.handle,
                TOKEN_ALL_ACCESS,
                None,
                SecurityImpersonation,
                TokenImpersonation,
                &mut impersonation,
            )
            .map_err(|error| WinError::api("DuplicateTokenEx", error))?;
            if let Err(error) = SetThreadToken(None, Some(impersonation)) {
                let _ = CloseHandle(impersonation);
                return Err(WinError::api("SetThreadToken", error));
            }
            Ok(Self {
                token: impersonation,
                active: true,
            })
        }
    }

    fn revert(&mut self) -> Result<(), WinError> {
        unsafe {
            RevertToSelf().map_err(|error| WinError::api("RevertToSelf", error))?;
            self.active = false;
            CloseHandle(self.token).map_err(|error| WinError::api("CloseHandle(token)", error))?;
            self.token = HANDLE::default();
            Ok(())
        }
    }
}

impl Drop for ImpersonationGuard {
    fn drop(&mut self) {
        unsafe {
            if self.active {
                let _ = RevertToSelf();
            }
            if !self.token.is_invalid() {
                let _ = CloseHandle(self.token);
            }
        }
    }
}

fn perform(operation: FsOperation) -> Result<(), WinError> {
    match operation {
        FsOperation::Write { path, bytes } => write_file(&path, &bytes),
        FsOperation::CreateDir { path, recursive } => create_dir(&path, recursive),
        FsOperation::Rename { from, to } => rename(&from, &to),
        FsOperation::Remove {
            path,
            recursive,
            is_dir,
        } => remove(&path, recursive, is_dir),
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), WinError> {
    let path = wide(path);
    unsafe {
        let handle = CreateFileW(
            PCWSTR(path.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|error| WinError::api("CreateFileW", error))?;
        let mut written = 0;
        let result = WriteFile(handle, Some(bytes), Some(&mut written), None)
            .map_err(|error| WinError::api("WriteFile", error));
        let _ = CloseHandle(handle);
        result?;
        if written != bytes.len() as u32 {
            return Err(WinError::setup(format!(
                "short WriteFile: {written}/{}",
                bytes.len()
            )));
        }
        Ok(())
    }
}

fn create_dir(path: &Path, recursive: bool) -> Result<(), WinError> {
    if !recursive {
        return create_one_dir(path);
    }
    let mut ancestors: Vec<&Path> = path
        .ancestors()
        .filter(|part| !part.as_os_str().is_empty())
        .collect();
    ancestors.reverse();
    for ancestor in ancestors {
        if let Err(error) = create_one_dir(ancestor) {
            if error.raw_code() != Some(ERROR_ALREADY_EXISTS.0) {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn create_one_dir(path: &Path) -> Result<(), WinError> {
    let path = wide(path);
    unsafe { CreateDirectoryW(PCWSTR(path.as_ptr()), None) }
        .map_err(|error| WinError::api("CreateDirectoryW", error))
}

fn rename(from: &Path, to: &Path) -> Result<(), WinError> {
    let from = wide(from);
    let to = wide(to);
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| WinError::api("MoveFileExW", error))
}

fn remove(path: &Path, recursive: bool, is_dir: bool) -> Result<(), WinError> {
    if recursive && is_dir {
        remove_tree(path)
    } else if is_dir {
        remove_dir(path)
    } else {
        delete_file(path)
    }
}

fn delete_file(path: &Path) -> Result<(), WinError> {
    let path = wide(path);
    unsafe { DeleteFileW(PCWSTR(path.as_ptr())) }
        .map_err(|error| WinError::api("DeleteFileW", error))
}

fn remove_dir(path: &Path) -> Result<(), WinError> {
    let path = wide(path);
    unsafe { RemoveDirectoryW(PCWSTR(path.as_ptr())) }
        .map_err(|error| WinError::api("RemoveDirectoryW", error))
}

fn remove_tree(path: &Path) -> Result<(), WinError> {
    let pattern = wide(&path.join("*"));
    unsafe {
        let mut data = WIN32_FIND_DATAW::default();
        let search = FindFirstFileW(PCWSTR(pattern.as_ptr()), &mut data)
            .map_err(|error| WinError::api("FindFirstFileW", error))?;
        loop {
            let length = data
                .cFileName
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(data.cFileName.len());
            use std::os::windows::ffi::OsStringExt;
            let name = OsString::from_wide(&data.cFileName[..length]);
            if name != "." && name != ".." {
                let child = path.join(name);
                let directory = data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                let reparse = data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
                let result = if directory && !reparse {
                    remove_tree(&child)
                } else if directory {
                    remove_dir(&child)
                } else {
                    delete_file(&child)
                };
                if let Err(error) = result {
                    let _ = FindClose(search);
                    return Err(error);
                }
            }
            match FindNextFileW(search, &mut data) {
                Ok(()) => {}
                Err(error) if raw_code(&error) == Some(ERROR_NO_MORE_FILES.0) => break,
                Err(error) => {
                    let _ = FindClose(search);
                    return Err(WinError::api("FindNextFileW", error));
                }
            }
        }
        FindClose(search).map_err(|error| WinError::api("FindClose", error))?;
    }
    remove_dir(path)
}

enum ProbeOutcome {
    Denied,
    Created,
}

fn probe_create(path: &Path) -> Result<ProbeOutcome, WinError> {
    let path = wide(path);
    unsafe {
        match CreateFileW(
            PCWSTR(path.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            None,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                Ok(ProbeOutcome::Created)
            }
            Err(error) if raw_code(&error) == Some(ERROR_ACCESS_DENIED.0) => {
                Ok(ProbeOutcome::Denied)
            }
            Err(error) => Err(WinError::api("CreateFileW(canary)", error)),
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn raw_code(error: &windows::core::Error) -> Option<u32> {
    let value = error.code();
    let bits = value.0 as u32;
    (bits & 0xffff_0000 == 0x8007_0000).then_some(bits & 0xffff)
}

#[derive(Debug)]
struct WinError {
    context: String,
    source: Option<windows::core::Error>,
}

impl WinError {
    fn api(context: &str, source: windows::core::Error) -> Self {
        Self {
            context: context.into(),
            source: Some(source),
        }
    }

    fn setup(context: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            source: None,
        }
    }

    fn raw_code(&self) -> Option<u32> {
        self.source.as_ref().and_then(raw_code)
    }
}

impl std::fmt::Display for WinError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(error) => write!(
                formatter,
                "{}: {} (0x{:08x})",
                self.context,
                error.message(),
                error.code().0 as u32
            ),
            None => formatter.write_str(&self.context),
        }
    }
}

impl From<WinError> for io::Error {
    fn from(error: WinError) -> Self {
        // Preserve the existing sink contract: any sandboxed mutation failure
        // is surfaced as PermissionDenied, with the Win32 API/code retained in
        // the message for diagnosis.
        io::Error::new(io::ErrorKind::PermissionDenied, error.to_string())
    }
}
