use std::mem::size_of;

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{LocalFree, GENERIC_ALL, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    SetEntriesInAclW, EXPLICIT_ACCESS_W, GRANT_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN,
    TRUSTEE_W,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, GetUserObjectSecurity, InitializeSecurityDescriptor,
    SetSecurityDescriptorDacl, SetUserObjectSecurity, ACL, DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_DESCRIPTOR,
};
use windows::Win32::System::StationsAndDesktops::{
    GetProcessWindowStation, GetThreadDesktop, DESKTOP_CREATEMENU, DESKTOP_CREATEWINDOW,
    DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL, DESKTOP_JOURNALPLAYBACK, DESKTOP_JOURNALRECORD,
    DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS,
};
use windows::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::WINSTA_ALL_ACCESS;

use super::token::RestrictedToken;

/// Restores the window-station and desktop DACLs after the sandboxed process
/// exits. The temporary ACE is intentionally scoped to the child's lifetime.
pub(super) struct DesktopAccessGuard {
    originals: Vec<OriginalSecurity>,
}

struct OriginalSecurity {
    object: HANDLE,
    descriptor: Box<[usize]>,
}

impl DesktopAccessGuard {
    pub(super) unsafe fn grant(token: &RestrictedToken) -> Result<Self, String> {
        let station = GetProcessWindowStation().map_err(winerr)?;
        let desktop = GetThreadDesktop(GetCurrentThreadId()).map_err(winerr)?;
        token.with_access_sids(|sids| unsafe {
            let mut originals = Vec::with_capacity(2);
            grant_object(
                HANDLE(station.0),
                sids,
                WINSTA_ALL_ACCESS as u32 | GENERIC_ALL.0,
                &mut originals,
            )?;
            let desktop_rights = DESKTOP_CREATEWINDOW.0
                | DESKTOP_CREATEMENU.0
                | DESKTOP_HOOKCONTROL.0
                | DESKTOP_JOURNALRECORD.0
                | DESKTOP_JOURNALPLAYBACK.0
                | DESKTOP_ENUMERATE.0
                | DESKTOP_WRITEOBJECTS.0
                | DESKTOP_SWITCHDESKTOP.0
                | DESKTOP_READOBJECTS.0
                | GENERIC_ALL.0;
            if let Err(error) =
                grant_object(HANDLE(desktop.0), sids, desktop_rights, &mut originals)
            {
                restore_all(&originals);
                return Err(error);
            }
            Ok(Self { originals })
        })
    }

    pub(super) unsafe fn grant_package(sid: PSID) -> Result<Self, String> {
        grant_sids(&[sid])
    }
}

unsafe fn grant_sids(sids: &[PSID]) -> Result<DesktopAccessGuard, String> {
    let station = GetProcessWindowStation().map_err(winerr)?;
    let desktop = GetThreadDesktop(GetCurrentThreadId()).map_err(winerr)?;
    let mut originals = Vec::with_capacity(2);
    grant_object(
        HANDLE(station.0),
        sids,
        WINSTA_ALL_ACCESS as u32 | GENERIC_ALL.0,
        &mut originals,
    )?;
    let desktop_rights = DESKTOP_CREATEWINDOW.0
        | DESKTOP_CREATEMENU.0
        | DESKTOP_HOOKCONTROL.0
        | DESKTOP_JOURNALRECORD.0
        | DESKTOP_JOURNALPLAYBACK.0
        | DESKTOP_ENUMERATE.0
        | DESKTOP_WRITEOBJECTS.0
        | DESKTOP_SWITCHDESKTOP.0
        | DESKTOP_READOBJECTS.0
        | GENERIC_ALL.0;
    if let Err(error) = grant_object(HANDLE(desktop.0), sids, desktop_rights, &mut originals) {
        restore_all(&originals);
        return Err(error);
    }
    Ok(DesktopAccessGuard { originals })
}

impl Drop for DesktopAccessGuard {
    fn drop(&mut self) {
        unsafe { restore_all(&self.originals) }
    }
}

unsafe fn grant_object(
    object: HANDLE,
    sids: &[PSID],
    permissions: u32,
    originals: &mut Vec<OriginalSecurity>,
) -> Result<(), String> {
    let original = read_security(object)?;
    let descriptor = PSECURITY_DESCRIPTOR(original.as_ptr() as *mut _);
    let mut present = BOOL::default();
    let mut defaulted = BOOL::default();
    let mut old_acl: *mut ACL = std::ptr::null_mut();
    GetSecurityDescriptorDacl(descriptor, &mut present, &mut old_acl, &mut defaulted)
        .map_err(winerr)?;
    if !present.as_bool() {
        return Err("window object has no DACL".into());
    }
    let entries: Vec<EXPLICIT_ACCESS_W> = sids
        .iter()
        .map(|sid| EXPLICIT_ACCESS_W {
            grfAccessPermissions: permissions,
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
    let mut new_acl: *mut ACL = std::ptr::null_mut();
    SetEntriesInAclW(Some(&entries), Some(old_acl), &mut new_acl)
        .ok()
        .map_err(winerr)?;
    let mut absolute = SECURITY_DESCRIPTOR::default();
    let absolute_ptr = PSECURITY_DESCRIPTOR((&mut absolute as *mut SECURITY_DESCRIPTOR).cast());
    let changed = InitializeSecurityDescriptor(absolute_ptr, SECURITY_DESCRIPTOR_REVISION)
        .and_then(|_| SetSecurityDescriptorDacl(absolute_ptr, true, Some(new_acl), false))
        .and_then(|_| SetUserObjectSecurity(object, &DACL_SECURITY_INFORMATION, absolute_ptr))
        .map_err(winerr);
    let _ = LocalFree(Some(HLOCAL(new_acl.cast())));
    changed?;
    originals.push(OriginalSecurity {
        object,
        descriptor: original,
    });
    Ok(())
}

unsafe fn read_security(object: HANDLE) -> Result<Box<[usize]>, String> {
    let mut needed = 0_u32;
    let requested = DACL_SECURITY_INFORMATION.0;
    let _ = GetUserObjectSecurity(object, &requested, None, 0, &mut needed);
    if needed == 0 {
        return Err("GetUserObjectSecurity returned no size".into());
    }
    // Store the self-relative SECURITY_DESCRIPTOR in machine-word storage so
    // its pointer has the alignment required by the Win32 security APIs.
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut descriptor = vec![0_usize; words].into_boxed_slice();
    GetUserObjectSecurity(
        object,
        &requested,
        Some(PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast())),
        needed,
        &mut needed,
    )
    .map_err(winerr)?;
    Ok(descriptor)
}

unsafe fn restore_all(originals: &[OriginalSecurity]) {
    for original in originals.iter().rev() {
        let descriptor = PSECURITY_DESCRIPTOR(original.descriptor.as_ptr() as *mut _);
        let _ = SetUserObjectSecurity(original.object, &DACL_SECURITY_INFORMATION, descriptor);
    }
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}
