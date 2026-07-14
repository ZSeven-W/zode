use std::mem::size_of;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, GENERIC_ALL, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, SetEntriesInAclW, EXPLICIT_ACCESS_W, GRANT_ACCESS, TRUSTEE_IS_SID,
    TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows::Win32::Security::{
    CreateRestrictedToken, GetTokenInformation, SetTokenInformation, TokenDefaultDacl, TokenGroups,
    TokenIntegrityLevel, TokenLogonSid, TokenPrivileges, TokenRestrictedSids, TokenUser, ACL,
    DISABLE_MAX_PRIVILEGE, PSID, SID_AND_ATTRIBUTES, TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL,
    TOKEN_GROUPS, TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_USER, WRITE_RESTRICTED,
};
use windows::Win32::System::SystemServices::{
    SE_GROUP_INTEGRITY, SE_GROUP_LOGON_ID, SE_GROUP_USE_FOR_DENY_ONLY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::sandbox::windows_policy::{capability_sid_components, normalize_windows_path};

pub(super) struct RestrictedToken {
    pub(super) handle: HANDLE,
    owned_sids: Vec<PSID>,
}

impl Drop for RestrictedToken {
    fn drop(&mut self) {
        unsafe {
            for sid in self.owned_sids.drain(..) {
                let _ = LocalFree(Some(HLOCAL(sid.0)));
            }
            let _ = CloseHandle(self.handle);
        }
    }
}

impl RestrictedToken {
    pub(super) fn create(roots: &[std::path::PathBuf], read_only: bool) -> Result<Self, String> {
        unsafe {
            let mut source = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut source).map_err(winerr)?;

            let group_storage = token_info(source, windows::Win32::Security::TokenGroups)?;
            let source_groups = &*(group_storage.as_ptr() as *const TOKEN_GROUPS);
            let source_group_slice = std::slice::from_raw_parts(
                source_groups.Groups.as_ptr(),
                source_groups.GroupCount as usize,
            );
            let powerful = [
                windows::Win32::Security::WinBuiltinAdministratorsSid,
                windows::Win32::Security::WinBuiltinPowerUsersSid,
                windows::Win32::Security::WinBuiltinAccountOperatorsSid,
                windows::Win32::Security::WinBuiltinSystemOperatorsSid,
                windows::Win32::Security::WinBuiltinPrintOperatorsSid,
                windows::Win32::Security::WinBuiltinBackupOperatorsSid,
                windows::Win32::Security::WinLocalSystemSid,
            ];
            let disabled: Vec<SID_AND_ATTRIBUTES> = source_group_slice
                .iter()
                .copied()
                .filter(|group| {
                    powerful.iter().any(|kind| {
                        windows::Win32::Security::IsWellKnownSid(group.Sid, *kind).as_bool()
                    })
                })
                .collect();
            let source_logon_sid = source_group_slice
                .iter()
                .find(|group| {
                    group.Attributes & SE_GROUP_LOGON_ID as u32 == SE_GROUP_LOGON_ID as u32
                })
                .map(|group| group.Sid)
                .ok_or_else(|| {
                    let _ = CloseHandle(source);
                    "source token has no logon SID; refusing weaker fallback".to_string()
                })?;

            let mut owned_sids = Vec::new();
            let mut restricting = Vec::new();
            for root in roots {
                let normalized = normalize_windows_path(root)?;
                let parts = capability_sid_components(&normalized);
                let sid = sid_from_string(&format!(
                    "S-1-5-113-{}-{}-{}-{}",
                    parts[0], parts[1], parts[2], parts[3]
                ))?;
                restricting.push(SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: 0,
                });
                owned_sids.push(sid);
            }
            if read_only {
                let sid = sid_from_string("S-1-5-113-0-0-0-1")?;
                restricting.push(SID_AND_ATTRIBUTES {
                    Sid: sid,
                    Attributes: 0,
                });
                owned_sids.push(sid);
            }

            // Larger Windows images can initialize DLLs, activation contexts,
            // and CRT state through session objects whose DACL names either
            // the logon SID or NT AUTHORITY\RESTRICTED. Include both standard
            // compatibility SIDs in the WRITE_RESTRICTED set so those objects
            // remain usable. Do not add Everyone or TokenUser: an Everyone ACE
            // would reopen world-writable files, while TokenUser would reopen
            // the user's ordinary files outside the capability roots.
            let restricted_sid = sid_from_string("S-1-5-12")?;
            restricting.push(SID_AND_ATTRIBUTES {
                Sid: restricted_sid,
                Attributes: 0,
            });
            owned_sids.push(restricted_sid);
            restricting.push(SID_AND_ATTRIBUTES {
                Sid: source_logon_sid,
                Attributes: 0,
            });

            // WRITE_RESTRICTED applies the restricting SID access check only
            // to write-like access. Normal reads remain available. Files in a
            // writable root name its synthetic capability SID; ordinary files
            // outside those roots generally name TokenUser, Administrators, or
            // Everyone, none of which satisfies this restricting set. A file
            // explicitly writable to the logon SID or RESTRICTED is a known
            // Tier 1 best-effort exception, not a claim of complete confinement.
            let mut handle = HANDLE::default();
            let result = CreateRestrictedToken(
                source,
                DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
                Some(&disabled),
                None,
                Some(&restricting),
                &mut handle,
            );
            let _ = CloseHandle(source);
            result.map_err(winerr)?;

            // Always set a deterministic integrity level so the sandbox never
            // inherits the parent's — an elevated/admin parent (e.g. the CI
            // runner) is HIGH integrity, which must not leak into the sandbox.
            // Read-only uses LOW; workspace-write clamps to MEDIUM (LOW would
            // block writes to ordinary medium-integrity roots despite their
            // capability ACE). Lowering a token's own integrity is always
            // permitted, so this succeeds from a high-integrity parent too.
            let integrity_sid = if read_only {
                "S-1-16-4096" // SECURITY_MANDATORY_LOW_RID
            } else {
                "S-1-16-8192" // SECURITY_MANDATORY_MEDIUM_RID
            };
            set_integrity(handle, &mut owned_sids, integrity_sid)?;
            set_restrictive_default_dacl(handle)?;
            verify(
                handle,
                expected_restricting_sid_count(roots.len(), read_only),
                read_only,
            )?;
            Ok(Self { handle, owned_sids })
        }
    }

    /// Calls `operation` while storage backing the selected token SID is live.
    /// The logon SID is preferred because it identifies this interactive
    /// session; TokenUser is the fallback for service/non-interactive tokens.
    pub(super) unsafe fn with_access_sids<T>(
        &self,
        operation: impl FnOnce(&[PSID]) -> Result<T, String>,
    ) -> Result<T, String> {
        let logon = token_info(self.handle, TokenLogonSid).ok();
        let user = token_info(self.handle, TokenUser)?;
        let restricted = token_info(self.handle, TokenRestrictedSids)?;
        let mut sids = Vec::new();
        if let Some(sid) = logon.as_deref().and_then(|bytes| logon_sid(bytes)) {
            sids.push(sid);
        } else {
            sids.push((*(user.as_ptr() as *const TOKEN_USER)).User.Sid);
        }
        let groups = &*(restricted.as_ptr() as *const TOKEN_GROUPS);
        sids.extend(
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
                .iter()
                .map(|group| group.Sid),
        );
        operation(&sids)
    }
}

fn expected_restricting_sid_count(root_count: usize, read_only: bool) -> usize {
    root_count + usize::from(read_only) + 2
}

unsafe fn set_integrity(
    handle: HANDLE,
    owned: &mut Vec<PSID>,
    integrity_sid: &str,
) -> Result<(), String> {
    let sid = sid_from_string(integrity_sid)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    let length =
        size_of::<TOKEN_MANDATORY_LABEL>() + windows::Win32::Security::GetLengthSid(sid) as usize;
    let result = SetTokenInformation(
        handle,
        TokenIntegrityLevel,
        &label as *const _ as *const _,
        length as u32,
    )
    .map_err(winerr);
    if result.is_ok() {
        owned.push(sid);
    } else {
        let _ = LocalFree(Some(HLOCAL(sid.0)));
    }
    result
}

unsafe fn set_restrictive_default_dacl(handle: HANDLE) -> Result<(), String> {
    let user = token_info(handle, TokenUser)?;
    let logon = token_info(handle, TokenLogonSid).ok();
    let restricted = token_info(handle, TokenRestrictedSids)?;
    let mut sids = vec![(*(user.as_ptr() as *const TOKEN_USER)).User.Sid];
    if let Some(sid) = logon.as_deref().and_then(|bytes| logon_sid(bytes)) {
        sids.push(sid);
    }
    let groups = &*(restricted.as_ptr() as *const TOKEN_GROUPS);
    sids.extend(
        std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
            .iter()
            .map(|group| group.Sid),
    );
    let entries: Vec<EXPLICIT_ACCESS_W> = sids
        .into_iter()
        .map(|sid| EXPLICIT_ACCESS_W {
            grfAccessPermissions: GENERIC_ALL.0,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: Default::default(),
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: Default::default(),
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_UNKNOWN,
                ptstrName: PWSTR(sid.0.cast()),
            },
        })
        .collect();
    let mut acl: *mut ACL = std::ptr::null_mut();
    SetEntriesInAclW(Some(&entries), None, &mut acl)
        .ok()
        .map_err(winerr)?;
    if acl.is_null() {
        return Err("restricted token default DACL is missing".into());
    }
    // WRITE_RESTRICTED checks object writes against both the normal token SIDs
    // and the restricting SIDs. Include the user, logon SID, and synthetic
    // restricting SIDs so objects created during DLL/CRT initialization remain
    // usable by the child without granting a broad SID such as Everyone.
    let default = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    let result = SetTokenInformation(
        handle,
        TokenDefaultDacl,
        &default as *const _ as *const _,
        size_of::<TOKEN_DEFAULT_DACL>() as u32,
    )
    .map_err(winerr);
    let _ = LocalFree(Some(HLOCAL(acl.cast())));
    result
}

unsafe fn logon_sid(storage: &[u8]) -> Option<PSID> {
    let groups = &*(storage.as_ptr() as *const TOKEN_GROUPS);
    std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        .iter()
        .find(|group| group.Attributes & SE_GROUP_LOGON_ID as u32 == SE_GROUP_LOGON_ID as u32)
        .map(|group| group.Sid)
}

unsafe fn verify(
    handle: HANDLE,
    expected_restricted: usize,
    expect_low_integrity: bool,
) -> Result<(), String> {
    let restricted = token_info(handle, TokenRestrictedSids)?;
    let groups = &*(restricted.as_ptr() as *const TOKEN_GROUPS);
    if groups.GroupCount as usize != expected_restricted {
        return Err(format!(
            "restricted token SID verification failed: expected {expected_restricted}, got {}",
            groups.GroupCount
        ));
    }
    let restricted_slice =
        std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
    if !restricted_slice.iter().any(|group| {
        windows::Win32::Security::IsWellKnownSid(
            group.Sid,
            windows::Win32::Security::WinRestrictedCodeSid,
        )
        .as_bool()
    }) {
        return Err("restricted token is missing NT AUTHORITY\\RESTRICTED".into());
    }
    let logon_storage = token_info(handle, TokenLogonSid)?;
    let expected_logon =
        logon_sid(&logon_storage).ok_or_else(|| "restricted token has no logon SID".to_string())?;
    if !restricted_slice
        .iter()
        .any(|group| windows::Win32::Security::EqualSid(group.Sid, expected_logon).is_ok())
    {
        return Err("restricted token restricting set is missing its logon SID".into());
    }
    let privileges = token_info(handle, TokenPrivileges)?;
    let privileges = &*(privileges.as_ptr() as *const TOKEN_PRIVILEGES);
    // DISABLE_MAX_PRIVILEGE retains only SeChangeNotifyPrivilege by contract.
    if privileges.PrivilegeCount != 1 {
        return Err(format!(
            "restricted token retained {} privileges; expected only SeChangeNotifyPrivilege",
            privileges.PrivilegeCount
        ));
    }
    {
        use std::os::windows::ffi::OsStrExt;
        let name: Vec<u16> = std::ffi::OsStr::new("SeChangeNotifyPrivilege")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut expected = windows::Win32::Foundation::LUID::default();
        windows::Win32::Security::LookupPrivilegeValueW(
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            &mut expected,
        )
        .map_err(winerr)?;
        let actual = privileges.Privileges[0].Luid;
        if actual.LowPart != expected.LowPart || actual.HighPart != expected.HighPart {
            return Err(
                "restricted token retained a privilege other than SeChangeNotifyPrivilege".into(),
            );
        }
    }
    let group_bytes = token_info(handle, TokenGroups)?;
    let groups = &*(group_bytes.as_ptr() as *const TOKEN_GROUPS);
    let group_slice =
        std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
    let powerful = [
        windows::Win32::Security::WinBuiltinAdministratorsSid,
        windows::Win32::Security::WinBuiltinPowerUsersSid,
        windows::Win32::Security::WinBuiltinAccountOperatorsSid,
        windows::Win32::Security::WinBuiltinSystemOperatorsSid,
        windows::Win32::Security::WinBuiltinPrintOperatorsSid,
        windows::Win32::Security::WinBuiltinBackupOperatorsSid,
        windows::Win32::Security::WinLocalSystemSid,
    ];
    for group in group_slice {
        let is_powerful = powerful
            .iter()
            .any(|kind| windows::Win32::Security::IsWellKnownSid(group.Sid, *kind).as_bool());
        if is_powerful && group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0 {
            return Err("powerful token group was not converted to deny-only".into());
        }
    }
    let integrity = token_info(handle, TokenIntegrityLevel)?;
    let label = &*(integrity.as_ptr() as *const TOKEN_MANDATORY_LABEL);
    let count = windows::Win32::Security::GetSidSubAuthorityCount(label.Label.Sid);
    if count.is_null() || *count == 0 {
        return Err("restricted token integrity SID is malformed".into());
    }
    let rid = *windows::Win32::Security::GetSidSubAuthority(label.Label.Sid, u32::from(*count - 1));
    if expect_low_integrity && rid > 4096 {
        return Err(format!("restricted token integrity is not low: RID {rid}"));
    }
    if rid > 8192 {
        return Err(format!(
            "restricted token unexpectedly has high integrity: RID {rid}"
        ));
    }
    let default = token_info(handle, TokenDefaultDacl)?;
    if (*(default.as_ptr() as *const TOKEN_DEFAULT_DACL))
        .DefaultDacl
        .is_null()
    {
        return Err("restricted token default DACL verification failed".into());
    }
    Ok(())
}

unsafe fn token_info(
    handle: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<u8>, String> {
    let mut needed = 0;
    let _ = GetTokenInformation(handle, class, None, 0, &mut needed);
    if needed == 0 {
        return Err(format!("GetTokenInformation({}) returned no size", class.0));
    }
    let mut bytes = vec![0_u8; needed as usize];
    GetTokenInformation(
        handle,
        class,
        Some(bytes.as_mut_ptr() as *mut _),
        needed,
        &mut needed,
    )
    .map_err(winerr)?;
    Ok(bytes)
}

unsafe fn sid_from_string(value: &str) -> Result<PSID, String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut sid = PSID::default();
    ConvertStringSidToSidW(PCWSTR(wide.as_ptr()), &mut sid).map_err(winerr)?;
    Ok(sid)
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}

#[cfg(test)]
mod tests {
    use super::expected_restricting_sid_count;

    #[test]
    fn restricting_count_includes_process_init_compatibility_sids() {
        assert_eq!(expected_restricting_sid_count(3, false), 5);
        assert_eq!(expected_restricting_sid_count(0, true), 3);
    }
}
