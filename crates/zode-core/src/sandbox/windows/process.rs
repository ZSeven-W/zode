use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::SearchPathW;
use windows::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use super::appcontainer::{AppContainerProfile, LowBoxToken};
use super::desktop::DesktopAccessGuard;
use super::token::RestrictedToken;
use crate::sandbox::windows_policy::WindowsPolicy;

pub(super) fn launch(policy: &WindowsPolicy, token: &RestrictedToken) -> Result<u32, String> {
    launch_inner(policy, token.handle, Some(token), None)
}

pub(super) fn launch_lowbox(
    policy: &WindowsPolicy,
    token: &LowBoxToken,
    profile: &AppContainerProfile,
) -> Result<u32, String> {
    // The token already carries its AppContainer identity. Supplying
    // PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as well would ask process
    // creation to synthesize a second lowbox identity, so the only extended
    // attribute used below remains the stdio handle allowlist.
    launch_inner(policy, token.handle, None, Some(profile))
}

fn launch_inner(
    policy: &WindowsPolicy,
    token_handle: windows::Win32::Foundation::HANDLE,
    restricted: Option<&RestrictedToken>,
    profile: Option<&AppContainerProfile>,
) -> Result<u32, String> {
    if policy.argv.is_empty() {
        return Err("sandbox policy argv is empty".into());
    }
    let application_path = resolve_application(&policy.argv[0])?;
    // Proxy variables remain advisory in Tier 1. Tier 2 is enforced by the
    // AppContainer's omission of every network capability.
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
    std::env::set_var("ALL_PROXY", "socks5://127.0.0.1:9");
    std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");
    std::env::set_var(
        "ZODE_SANDBOX_NETWORK",
        if policy.network_enforced {
            "denied-appcontainer"
        } else {
            "unenforced"
        },
    );

    unsafe {
        // GUI-capable executables can connect to WinSta0\Default during DLL
        // initialization even when the program itself is console-only. Keep a
        // narrowly scoped ACE for the restricted token's logon/user SID until
        // the child exits, then restore both original DACLs.
        let _desktop_access = match (restricted, profile) {
            (Some(token), None) => DesktopAccessGuard::grant(token)?,
            (None, Some(profile)) => DesktopAccessGuard::grant_package(profile.sid)?,
            _ => return Err("invalid Windows sandbox launcher configuration".into()),
        };
        let handles = [
            GetStdHandle(STD_INPUT_HANDLE).map_err(winerr)?,
            GetStdHandle(STD_OUTPUT_HANDLE).map_err(winerr)?,
            GetStdHandle(STD_ERROR_HANDLE).map_err(winerr)?,
        ];
        for handle in handles {
            if !handle.is_invalid() {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT)
                    .map_err(winerr)?;
            }
        }

        let mut attribute_bytes = 0_usize;
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut attribute_bytes);
        if attribute_bytes == 0 {
            return Err("InitializeProcThreadAttributeList returned no size".into());
        }
        let mut storage = vec![0_u8; attribute_bytes];
        let list = windows::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST(
            storage.as_mut_ptr() as *mut _,
        );
        InitializeProcThreadAttributeList(Some(list), 1, None, &mut attribute_bytes)
            .map_err(winerr)?;
        let attribute_result = UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(handles.as_ptr() as *const _),
            size_of_val(&handles),
            None,
            None,
        );
        if let Err(error) = attribute_result {
            DeleteProcThreadAttributeList(list);
            return Err(winerr(error));
        }

        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        let mut desktop_name = wide(std::ffi::OsStr::new("WinSta0\\Default"));
        startup.StartupInfo.lpDesktop = PWSTR(desktop_name.as_mut_ptr());
        startup.lpAttributeList = list;
        let mut command = command_line(&policy.argv);
        // cmd.exe rejects an extended-length (`\\?\`) current directory.
        // Tier 1 keeps its proven path unchanged. For lowbox launches remove
        // only that prefix: lexical policy normalization is intentionally not
        // used here because lpCurrentDirectory must preserve the exact name of
        // the existing directory, including short-name and dot components.
        let appcontainer_cwd = profile
            .map(|_| policy.cwd.to_string_lossy())
            .map(|path| crate::sandbox::windows_policy::strip_verbatim_prefix(&path).to_owned());
        let cwd = match appcontainer_cwd.as_deref() {
            Some(path) => wide(std::ffi::OsStr::new(path)),
            None => wide(policy.cwd.as_os_str()),
        };
        let application = wide(application_path.as_os_str());
        let mut info: PROCESS_INFORMATION = zeroed();
        let created = CreateProcessAsUserW(
            Some(token_handle),
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            None,
            PCWSTR(cwd.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        );
        DeleteProcThreadAttributeList(list);
        for handle in handles {
            if !handle.is_invalid() {
                let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, Default::default());
            }
        }
        created.map_err(winerr)?;

        let job = CreateJobObjectW(None, PCWSTR::null()).map_err(|error| {
            let _ = CloseHandle(info.hThread);
            let _ = CloseHandle(info.hProcess);
            winerr(error)
        })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .and_then(|_| AssignProcessToJobObject(job, info.hProcess));
        if let Err(error) = configured {
            let _ = windows::Win32::System::Threading::TerminateProcess(info.hProcess, 1);
            let _ = CloseHandle(info.hThread);
            let _ = CloseHandle(info.hProcess);
            let _ = CloseHandle(job);
            return Err(winerr(error));
        }
        if ResumeThread(info.hThread) == u32::MAX {
            let _ = windows::Win32::System::Threading::TerminateProcess(info.hProcess, 1);
            let _ = CloseHandle(info.hThread);
            let _ = CloseHandle(info.hProcess);
            let _ = CloseHandle(job);
            return Err("ResumeThread failed".into());
        }
        let _ = CloseHandle(info.hThread);
        let wait = WaitForSingleObject(info.hProcess, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(info.hProcess);
            let _ = CloseHandle(job);
            return Err(format!("WaitForSingleObject returned {}", wait.0));
        }
        let mut code = 0_u32;
        GetExitCodeProcess(info.hProcess, &mut code).map_err(winerr)?;
        let _ = CloseHandle(info.hProcess);
        // Closing the job kills any descendants that attempted to outlive the
        // direct child. Ctrl+C naturally reaches the shared console; closing
        // this wrapper also triggers KILL_ON_JOB_CLOSE.
        let _ = CloseHandle(job);
        Ok(code)
    }
}

fn resolve_application(program: &str) -> Result<PathBuf, String> {
    let path = Path::new(program);
    if path.is_absolute() || path.exists() || program.contains('\\') || program.contains('/') {
        return Ok(path.to_path_buf());
    }
    if let Some(resolved) = search_path(path, None)? {
        return Ok(resolved);
    }
    if path.extension().is_none() {
        let extensions =
            std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        for extension in extensions.to_string_lossy().split(';') {
            if !extension.is_empty() {
                if let Some(resolved) = search_path(path, Some(extension))? {
                    return Ok(resolved);
                }
            }
        }
    }
    Err(format!(
        "resolve sandbox executable {program:?}: file not found on PATH"
    ))
}

fn search_path(path: &Path, extension: Option<&str>) -> Result<Option<PathBuf>, String> {
    let filename = wide(path.as_os_str());
    let extension = extension.map(|value| wide(std::ffi::OsStr::new(value)));
    let extension_ptr = extension
        .as_ref()
        .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr()));
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe {
        SearchPathW(
            PCWSTR::null(),
            PCWSTR(filename.as_ptr()),
            extension_ptr,
            Some(&mut buffer),
            None,
        )
    };
    if length == 0 {
        return Ok(None);
    }
    if length as usize >= buffer.len() {
        return Err(format!(
            "resolved sandbox executable path is too long: {length} UTF-16 units"
        ));
    }
    unsafe {
        use std::os::windows::ffi::OsStringExt;
        Ok(Some(
            std::ffi::OsString::from_wide(&buffer[..length as usize]).into(),
        ))
    }
}

fn command_line(argv: &[String]) -> Vec<u16> {
    let joined = argv
        .iter()
        .map(|arg| quote_windows_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    wide(std::ffi::OsStr::new(&joined))
}

fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.bytes().any(|byte| matches!(byte, b' ' | b'\t' | b'"')) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                slashes = 0;
                out.push(ch);
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

fn winerr(error: windows::core::Error) -> String {
    format!("{} (0x{:08x})", error.message(), error.code().0 as u32)
}

#[cfg(test)]
mod tests {
    use super::{quote_windows_arg, resolve_application};

    #[test]
    fn quotes_command_line_backslashes_before_quotes() {
        assert_eq!(quote_windows_arg("plain"), "plain");
        assert_eq!(quote_windows_arg("two words"), "\"two words\"");
        // MSVC/CommandLineToArgvW encoding of `a\"b`: the whole arg is wrapped
        // in bare quotes, the literal `"` becomes `\"`, and the backslash that
        // precedes it is doubled — yielding `"a\\\"b"` (not an escaped outer
        // quote).
        assert_eq!(quote_windows_arg(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn resolves_bare_application_through_windows_search_path() {
        let resolved = resolve_application("cmd.exe").expect("cmd.exe should be on PATH");
        assert!(
            resolved.is_absolute(),
            "resolved path: {}",
            resolved.display()
        );
        assert!(resolved.exists(), "resolved path: {}", resolved.display());
    }
}
