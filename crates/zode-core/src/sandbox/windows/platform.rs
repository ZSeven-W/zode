use std::path::Path;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
};

use super::acl::AppliedAcls;
use super::appcontainer::AppContainerProfile;
use super::process;
use super::token::RestrictedToken;
use crate::sandbox::windows_policy::WindowsPolicy;

pub(super) fn protect_policy_file(path: &Path) -> Result<(), String> {
    // Protected DACL: only the object's owner receives full access. The policy
    // contains argv and roots rather than credentials, but it must not be
    // replaceable between wrapper construction and self-reexec.
    unsafe {
        let sddl = wide(std::ffi::OsStr::new("D:P(A;;FA;;;OW)"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(winerr)?;
        let mut present = BOOL::default();
        let mut defaulted = BOOL::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            .map_err(winerr)?;
        if !present.as_bool() || dacl.is_null() {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            return Err("owner-only policy DACL is missing".into());
        }
        let path_w = wide(path.as_os_str());
        let result = SetNamedSecurityInfoW(
            PWSTR(path_w.as_ptr() as *mut u16),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
        .ok()
        .map_err(winerr);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
}

pub(super) fn launch_restricted(policy: &WindowsPolicy) -> Result<u32, String> {
    let _acls = AppliedAcls::apply(&policy.writable_roots)?;
    let token = RestrictedToken::create(&policy.writable_roots, policy.read_only)?;
    if policy.network_enforced {
        let profile = AppContainerProfile::open_or_create()?;
        // The lowbox package identity participates in write checks at the
        // target. Retained user SIDs already traverse the ancestor chain, so
        // this inheritable grant is limited to configured writable roots.
        let _package_acls =
            AppliedAcls::apply_package(&policy.writable_roots, profile.sid_string()?)?;
        let lowbox = profile.create_lowbox(&token)?;
        return process::launch_lowbox(policy, &lowbox, &profile);
    }
    process::launch(policy, &token)
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}
