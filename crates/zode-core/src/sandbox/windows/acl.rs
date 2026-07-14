use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, REVOKE_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows::Win32::Security::{
    ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
    PSECURITY_DESCRIPTOR, PSID,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_DELETE_CHILD,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
};

use crate::sandbox::windows_policy::{capability_sid_components, normalize_windows_path};

#[derive(Debug, Serialize, Deserialize)]
struct JournalEntry {
    root: PathBuf,
    sid: String,
    #[serde(default)]
    permissions: u32,
}

pub(super) struct AppliedAcls {
    entries: Vec<JournalEntry>,
    journal: PathBuf,
}

impl AppliedAcls {
    pub(super) fn apply(roots: &[PathBuf]) -> Result<Self, String> {
        Self::apply_sid(roots, capability_sids(roots)?, "")
    }

    pub(super) fn apply_package(roots: &[PathBuf], sid: String) -> Result<Self, String> {
        Self::apply_sid(
            roots,
            roots.iter().map(|_| sid.clone()).collect(),
            "package-",
        )
    }

    fn apply_sid(roots: &[PathBuf], sids: Vec<String>, journal_kind: &str) -> Result<Self, String> {
        let mut entries: Vec<JournalEntry> = Vec::with_capacity(roots.len());
        for (root, sid) in roots.iter().zip(sids) {
            let permissions = if sid.starts_with("S-1-15-2-") {
                package_permissions()
            } else {
                capability_permissions()
            };
            if let Err(error) = apply_entry(root, &sid, GRANT_ACCESS, permissions) {
                for prior in entries.iter().rev() {
                    let _ = apply_entry(
                        &prior.root,
                        &prior.sid,
                        REVOKE_ACCESS,
                        windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS(prior.permissions),
                    );
                }
                return Err(error);
            }
            entries.push(JournalEntry {
                root: root.clone(),
                sid,
                permissions: permissions.0,
            });
        }
        let journal = journal_path(journal_kind);
        let bytes = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
        std::fs::write(&journal, bytes).map_err(|e| format!("write ACL journal: {e}"))?;
        let guard = Self { entries, journal };
        super::platform::protect_policy_file(&guard.journal)?;
        Ok(guard)
    }
}

impl Drop for AppliedAcls {
    fn drop(&mut self) {
        for entry in self.entries.iter().rev() {
            let _ = apply_entry(
                &entry.root,
                &entry.sid,
                REVOKE_ACCESS,
                windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS(entry.permissions),
            );
        }
        let _ = std::fs::remove_file(&self.journal);
    }
}

pub fn cleanup_journal() -> Result<(), String> {
    let temp = std::env::temp_dir();
    for entry in std::fs::read_dir(&temp).map_err(|e| format!("read temp directory: {e}"))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("zode-sandbox-acl-") && name.ends_with(".json"));
        if matches {
            cleanup_file(&path)?;
        }
    }
    Ok(())
}

fn cleanup_file(journal: &Path) -> Result<(), String> {
    let bytes = match std::fs::read(&journal) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read ACL journal: {error}")),
    };
    let entries: Vec<JournalEntry> =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid ACL cleanup journal: {e}"))?;
    for entry in entries.iter().rev() {
        if !entry.sid.starts_with("S-1-5-113-") && !entry.sid.starts_with("S-1-15-2-") {
            return Err("ACL cleanup journal contains a non-zode SID".into());
        }
        normalize_windows_path(&entry.root)?;
        apply_entry(
            &entry.root,
            &entry.sid,
            REVOKE_ACCESS,
            windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS(entry.permissions),
        )?;
    }
    std::fs::remove_file(journal).map_err(|e| format!("remove ACL journal: {e}"))
}

fn journal_path(kind: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zode-sandbox-acl-{kind}{}.json",
        std::process::id()
    ))
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

fn apply_entry(
    root: &Path,
    sid_text: &str,
    mode: windows::Win32::Security::Authorization::ACCESS_MODE,
    permissions: windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS,
) -> Result<(), String> {
    let root_w = wide(root.as_os_str());
    let sid_w = wide(std::ffi::OsStr::new(sid_text));
    unsafe {
        let root_handle = CreateFileW(
            PCWSTR(root_w.as_ptr()),
            (READ_CONTROL | WRITE_DAC).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
        .map_err(winerr)?;
        let mut sid = PSID::default();
        if let Err(error) = ConvertStringSidToSidW(PCWSTR(sid_w.as_ptr()), &mut sid) {
            let _ = CloseHandle(root_handle);
            return Err(winerr(error));
        }

        let mut old_acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let get = GetSecurityInfo(
            root_handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut old_acl),
            None,
            Some(&mut descriptor),
        )
        .ok();
        if let Err(error) = get {
            let _ = CloseHandle(root_handle);
            let _ = LocalFree(Some(HLOCAL(sid.0)));
            return Err(winerr(error));
        }

        // FILE_DELETE_CHILD is deliberately retained for build tools and
        // atomic rename compatibility. Consequently neither Tier 1 nor Tier 2
        // can protect
        // `.git`/`.zode` from rename/delete through this parent; file tools
        // still reject those paths independently.
        let trustee = TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: Default::default(),
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: PWSTR(sid.0 as *mut u16),
        };
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: permissions.0,
            grfAccessMode: mode,
            grfInheritance: OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
            Trustee: trustee,
        };
        let mut new_acl: *mut ACL = std::ptr::null_mut();
        let result = SetEntriesInAclW(Some(&[entry]), Some(old_acl), &mut new_acl)
            .ok()
            .and_then(|_| {
                SetSecurityInfo(
                    root_handle,
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(new_acl),
                    None,
                )
                .ok()
            })
            .map_err(winerr);
        if !new_acl.is_null() {
            let _ = LocalFree(Some(HLOCAL(new_acl.cast())));
        }
        if !descriptor.0.is_null() {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        let _ = LocalFree(Some(HLOCAL(sid.0)));
        let _ = CloseHandle(root_handle);
        result
    }
}

fn capability_sids(roots: &[PathBuf]) -> Result<Vec<String>, String> {
    roots
        .iter()
        .map(|root| {
            let normalized = normalize_windows_path(root)?;
            let parts = capability_sid_components(&normalized);
            Ok(format!(
                "S-1-5-113-{}-{}-{}-{}",
                parts[0], parts[1], parts[2], parts[3]
            ))
        })
        .collect()
}

fn capability_permissions() -> windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS {
    FILE_GENERIC_READ
        | FILE_GENERIC_WRITE
        | FILE_ADD_FILE
        | FILE_ADD_SUBDIRECTORY
        | FILE_DELETE_CHILD
        | DELETE
}

fn package_permissions() -> windows::Win32::Storage::FileSystem::FILE_ACCESS_RIGHTS {
    capability_permissions() | FILE_GENERIC_EXECUTE
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}
