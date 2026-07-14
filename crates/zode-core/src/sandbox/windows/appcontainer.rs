use windows::core::{PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{FreeSid, PSID, SID_AND_ATTRIBUTES, TOKEN_ALL_ACCESS};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use super::token::RestrictedToken;

pub(super) const PROFILE_NAME: &str = "zode.sandbox.tier2";

pub(super) struct AppContainerProfile {
    pub(super) sid: PSID,
}

pub(super) struct LowBoxToken {
    pub(super) handle: HANDLE,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *mut core::ffi::c_void,
    attributes: u32,
    security_descriptor: *mut core::ffi::c_void,
    security_quality_of_service: *mut core::ffi::c_void,
}

impl AppContainerProfile {
    pub(super) fn open_or_create() -> Result<Self, String> {
        let name = wide(PROFILE_NAME);
        let display = wide("zode Tier 2 sandbox");
        let description = wide("Network-isolated zode command sandbox");
        unsafe {
            let sid = match CreateAppContainerProfile(
                PCWSTR(name.as_ptr()),
                PCWSTR(display.as_ptr()),
                PCWSTR(description.as_ptr()),
                None,
            ) {
                Ok(sid) => sid,
                Err(error) if error.code().0 as u32 == 0x8007_00b7 => {
                    DeriveAppContainerSidFromAppContainerName(PCWSTR(name.as_ptr()))
                        .map_err(winerr)?
                }
                Err(error) => return Err(winerr(error)),
            };
            Ok(Self { sid })
        }
    }

    pub(super) fn create_lowbox(&self, base: &RestrictedToken) -> Result<LowBoxToken, String> {
        // NtCreateLowBoxToken is exported by ntdll but has no supported Win32
        // import library. Its native signature is therefore resolved at run
        // time. The lowbox is built over the already-hardened Tier 1 primary
        // token: its normal user SIDs retain read/traverse access, its
        // restricting SIDs continue to gate writes, and the package SID with
        // an empty capability array supplies AppContainer network isolation.
        type NtCreateLowBoxToken = unsafe extern "system" fn(
            *mut HANDLE,
            HANDLE,
            u32,
            *const core::ffi::c_void,
            PSID,
            u32,
            *const SID_AND_ATTRIBUTES,
            u32,
            *const HANDLE,
        ) -> i32;
        unsafe {
            let ntdll = wide("ntdll.dll");
            let module = GetModuleHandleW(PCWSTR(ntdll.as_ptr())).map_err(winerr)?;
            let address = GetProcAddress(module, PCSTR(b"NtCreateLowBoxToken\0".as_ptr()))
                .ok_or_else(|| "NtCreateLowBoxToken is unavailable in ntdll".to_string())?;
            let create: NtCreateLowBoxToken = std::mem::transmute(address);
            let mut handle = HANDLE::default();
            let attributes = ObjectAttributes {
                length: std::mem::size_of::<ObjectAttributes>() as u32,
                root_directory: HANDLE::default(),
                object_name: std::ptr::null_mut(),
                attributes: 0,
                security_descriptor: std::ptr::null_mut(),
                security_quality_of_service: std::ptr::null_mut(),
            };
            let status = create(
                &mut handle,
                base.handle,
                TOKEN_ALL_ACCESS.0,
                (&attributes as *const ObjectAttributes).cast(),
                self.sid,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
            if status < 0 {
                return Err(format!(
                    "NtCreateLowBoxToken failed with NTSTATUS 0x{:08x}",
                    status as u32
                ));
            }
            if handle.is_invalid() {
                return Err("NtCreateLowBoxToken returned an invalid handle".into());
            }
            Ok(LowBoxToken { handle })
        }
    }

    pub(super) fn sid_string(&self) -> Result<String, String> {
        unsafe {
            let mut value = PWSTR(std::ptr::null_mut());
            ConvertSidToStringSidW(self.sid, &mut value).map_err(winerr)?;
            let result = value.to_string().map_err(|error| error.to_string());
            let _ = LocalFree(Some(HLOCAL(value.0.cast())));
            result
        }
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeSid(self.sid);
        }
    }
}

impl Drop for LowBoxToken {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub fn delete_profile() -> Result<(), String> {
    let name = wide(PROFILE_NAME);
    unsafe {
        match DeleteAppContainerProfile(PCWSTR(name.as_ptr())) {
            Ok(()) => Ok(()),
            Err(error) if error.code().0 as u32 == 0x8007_0002 => Ok(()),
            Err(error) => Err(winerr(error)),
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}
