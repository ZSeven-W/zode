//! Chrome Native Messaging registration for starting the bridge on demand.
//!
//! The native host is the zode executable itself. Chrome launches it with the
//! extension origin as argv[1]; the binary recognizes that exact invocation
//! before normal CLI parsing and enters the extension-only daemon mode.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ConfigManager;
use crate::CoreError;

use super::server::EXTENSION_ID;

pub const HOST_NAME: &str = "ai.zode.browser_bridge";
const STATE_FILE: &str = "browser-native-host.json";

#[derive(Debug, Serialize)]
struct NativeHostManifest<'a> {
    name: &'a str,
    description: &'a str,
    path: &'a Path,
    #[serde(rename = "type")]
    transport: &'a str,
    allowed_origins: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NativeHostState {
    cwd: PathBuf,
}

pub fn extension_origin() -> String {
    format!("chrome-extension://{EXTENSION_ID}/")
}

/// Every origin the native-messaging manifest allows: the developer-keyed
/// extension plus registered extras (the Chrome-Web-Store-published ID).
pub fn extension_origins() -> Vec<String> {
    super::server::allowed_extension_ids()
        .into_iter()
        .map(|id| format!("chrome-extension://{id}/"))
        .collect()
}

/// Chrome passes the caller origin as the first argument to a native host.
/// Shape-checked (any `chrome-extension://<id>/` origin) rather than matched
/// against the configured ID list: this runs BEFORE config loads, and it only
/// selects daemon mode — real access is still gated by the native-messaging
/// manifest's `allowed_origins` and the WebSocket token + Origin check.
pub fn is_invocation_arg(value: &OsStr) -> bool {
    let Some(s) = value.to_str() else {
        return false;
    };
    s.strip_prefix("chrome-extension://")
        .and_then(|rest| rest.strip_suffix('/'))
        .is_some_and(super::server::is_extension_id)
}

/// Register the running zode binary for Chrome/Chromium/Edge and remember the
/// workspace that should back new side-panel tasks when Chrome starts it.
pub fn install(cwd: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let executable = std::env::current_exe()?;
    let config_dir = ConfigManager::config_dir()?;
    let roots = platform_manifest_roots()?;
    install_into(&config_dir, &roots, &executable, cwd)
}

pub fn preferred_cwd() -> Option<PathBuf> {
    let path = ConfigManager::config_dir().ok()?.join(STATE_FILE);
    let state: NativeHostState = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    state.cwd.is_dir().then_some(state.cwd)
}

fn install_into(
    config_dir: &Path,
    manifest_roots: &[PathBuf],
    executable: &Path,
    cwd: &Path,
) -> Result<Vec<PathBuf>, CoreError> {
    if !executable.is_absolute() {
        return Err(CoreError::Other(
            "native messaging host executable must be absolute".into(),
        ));
    }
    std::fs::create_dir_all(config_dir)?;
    write_private_json(
        &config_dir.join(STATE_FILE),
        &NativeHostState {
            cwd: cwd.to_path_buf(),
        },
    )?;

    let manifest = NativeHostManifest {
        name: HOST_NAME,
        description: "Start the local zode browser bridge on demand",
        path: executable,
        transport: "stdio",
        allowed_origins: extension_origins(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut installed = Vec::new();
    for root in manifest_roots {
        std::fs::create_dir_all(root)?;
        let path = root.join(format!("{HOST_NAME}.json"));
        write_private(&path, &bytes)?;
        installed.push(path);
    }

    #[cfg(windows)]
    register_windows_manifests(&installed)?;

    Ok(installed)
}

fn platform_manifest_roots() -> Result<Vec<PathBuf>, CoreError> {
    let home =
        dirs::home_dir().ok_or_else(|| CoreError::Other("cannot resolve home directory".into()))?;
    #[cfg(target_os = "macos")]
    {
        let support = home.join("Library/Application Support");
        return Ok(vec![
            support.join("Google/Chrome/NativeMessagingHosts"),
            support.join("Chromium/NativeMessagingHosts"),
            support.join("Microsoft Edge/NativeMessagingHosts"),
        ]);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return Ok(vec![
            home.join(".config/google-chrome/NativeMessagingHosts"),
            home.join(".config/chromium/NativeMessagingHosts"),
            home.join(".config/microsoft-edge/NativeMessagingHosts"),
        ]);
    }
    #[cfg(windows)]
    {
        return Ok(vec![ConfigManager::config_dir()?.join("native-messaging")]);
    }
    #[allow(unreachable_code)]
    Err(CoreError::Other(
        "Chrome Native Messaging is unsupported on this platform".into(),
    ))
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<(), CoreError> {
    write_private(path, &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write as _;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(windows)]
fn register_windows_manifests(installed: &[PathBuf]) -> Result<(), CoreError> {
    let Some(manifest) = installed.first() else {
        return Err(CoreError::Other(
            "native host manifest was not written".into(),
        ));
    };
    for vendor in ["Google\\Chrome", "Chromium", "Microsoft\\Edge"] {
        let key = format!("HKCU\\Software\\{vendor}\\NativeMessagingHosts\\{HOST_NAME}");
        let status = std::process::Command::new("reg.exe")
            .args(["add", &key, "/ve", "/t", "REG_SZ", "/d"])
            .arg(manifest)
            .arg("/f")
            .status()?;
        if !status.success() {
            return Err(CoreError::Other(format!(
                "failed to register native messaging host at {key}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial(bridge_extension_ids)]
    fn manifest_and_workspace_state_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let roots = [dir.path().join("chrome-hosts")];
        let executable = dir.path().join("bin/zode");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"").unwrap();
        let cwd = dir.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();

        // The accept list is process-global; pin it to the default so a
        // concurrent bridge test can't leave a store id installed.
        crate::browser::bridge::server::set_allowed_extension_ids(vec![
            crate::browser::bridge::server::EXTENSION_ID.to_string(),
        ]);
        let installed = install_into(&config, &roots, &executable, &cwd).unwrap();
        assert_eq!(installed, vec![roots[0].join(format!("{HOST_NAME}.json"))]);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&installed[0]).unwrap()).unwrap();
        assert_eq!(manifest["name"], HOST_NAME);
        assert_eq!(manifest["path"], executable.display().to_string());
        assert_eq!(manifest["type"], "stdio");
        assert_eq!(manifest["allowed_origins"][0], extension_origin());

        let state: NativeHostState =
            serde_json::from_slice(&std::fs::read(config.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(state.cwd, cwd);
    }

    #[test]
    fn invocation_requires_the_exact_extension_origin() {
        assert!(is_invocation_arg(OsStr::new(&extension_origin())));
        assert!(!is_invocation_arg(OsStr::new("chrome-extension://wrong/")));
    }
}
