//! Experimental Windows Tier 1 runner.
//!
//! Tier 1 is best-effort write confinement, not a complete Windows security
//! boundary. Network is unenforced. Writable roots retain delete-child access
//! for build tools and atomic renames, so child deny ACEs cannot kernel-protect
//! `.git` or `.zode` against rename/delete through their parent.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::CoreError;

use super::windows_policy::{
    decode_policy, encode_policy, tier_one_summary, tier_two_summary, WindowsPolicy,
};
use super::{SandboxConfig, SandboxMode};

const PRIVATE_RUN: &str = "__sandbox-run";

mod acl;
mod appcontainer;
mod desktop;
#[path = "windows/fs-impersonation.rs"]
mod fs_impersonation;
mod process;
mod token;

pub(super) use fs_impersonation::FsOperation;

pub fn prepare_argv(config: &SandboxConfig, argv: &[String]) -> Result<Vec<String>, String> {
    let network_enforced = config.windows_network_enforced()?;
    let policy = WindowsPolicy {
        argv: argv.to_vec(),
        cwd: config.cwd.clone(),
        writable_roots: config.windows_writable_roots(),
        read_only: config.mode == SandboxMode::ReadOnly,
        network_enforced,
    };
    let path = write_protected_policy(&encode_policy(&policy)?)?;
    #[cfg(feature = "sandbox-test-runner")]
    let exe = std::env::var_os("ZODE_SANDBOX_RUNNER")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().map_err(|e| e.to_string()))
        .map_err(|e| format!("locate zode executable: {e}"))?;
    #[cfg(not(feature = "sandbox-test-runner"))]
    let exe = std::env::current_exe().map_err(|e| format!("locate zode executable: {e}"))?;
    Ok(vec![
        exe.to_string_lossy().into_owned(),
        PRIVATE_RUN.into(),
        "--policy-file".into(),
        path.to_string_lossy().into_owned(),
    ])
}

pub fn intercept_private_entrypoint(args: &[OsString]) -> Option<u32> {
    match args.get(1).and_then(|value| value.to_str()) {
        Some(PRIVATE_RUN) => Some(run_policy_entrypoint(args)),
        _ => None,
    }
}

pub async fn verify(config: &SandboxConfig) -> Result<(), CoreError> {
    let tier_two = config
        .windows_network_enforced()
        .map_err(CoreError::Other)?;
    eprintln!(
        "zode: {}",
        if tier_two {
            tier_two_summary(config.mode == SandboxMode::ReadOnly)
        } else {
            tier_one_summary(config.mode == SandboxMode::ReadOnly)
        }
    );
    let canary = config
        .windows_canary_path()
        .ok_or_else(|| CoreError::Other("no safe Windows sandbox canary path exists".into()))?;
    fs_impersonation::verify_write_denied(config, canary)
        .await
        .map_err(CoreError::Other)?;
    if tier_two {
        verify_appcontainer_network(config)
            .await
            .map_err(CoreError::Other)?;
    }
    Ok(())
}

async fn verify_appcontainer_network(config: &SandboxConfig) -> Result<(), String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("bind AppContainer loopback probe: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("configure AppContainer loopback probe: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let policy = WindowsPolicy {
        argv: vec![
            "curl.exe".into(),
            "-sS".into(),
            "-m".into(),
            "2".into(),
            "--noproxy".into(),
            "*".into(),
            "-o".into(),
            "NUL".into(),
            format!("http://127.0.0.1:{port}/"),
        ],
        cwd: config.cwd.clone(),
        writable_roots: config.windows_writable_roots(),
        read_only: config.mode == SandboxMode::ReadOnly,
        network_enforced: true,
    };
    let code = tokio::task::spawn_blocking(move || platform::launch_restricted(&policy))
        .await
        .map_err(|error| format!("AppContainer network probe worker failed: {error}"))??;
    let loopback_leaked = listener.accept().is_ok();
    if code != 0 && !loopback_leaked {
        Ok(())
    } else {
        Err(format!(
            "Windows Tier 2 AppContainer curl probe escaped or could not prove denial (exit {code}, loopback_leaked={loopback_leaked})"
        ))
    }
}

fn write_protected_policy(bytes: &[u8]) -> Result<PathBuf, String> {
    for attempt in 0..64_u32 {
        let path = std::env::temp_dir().join(format!(
            "zode-sandbox-policy-{}-{attempt}.json",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)
                    .map_err(|e| format!("write sandbox policy: {e}"))?;
                file.sync_all()
                    .map_err(|e| format!("sync sandbox policy: {e}"))?;
                protect_policy_file(&path)?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create sandbox policy: {error}")),
        }
    }
    Err("could not allocate a unique sandbox policy file".into())
}

fn run_policy_entrypoint(args: &[OsString]) -> u32 {
    let Some(path) = args.get(3).map(PathBuf::from) else {
        eprintln!("zode: missing sandbox policy file");
        return 2;
    };
    let result = (|| -> Result<u32, String> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| format!("open sandbox policy: {e}"))?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take((super::windows_policy::MAX_POLICY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read sandbox policy: {e}"))?;
        drop(file);
        std::fs::remove_file(&path).map_err(|e| format!("remove sandbox policy: {e}"))?;
        launch_restricted(&decode_policy(&bytes)?)
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("zode: Windows sandbox runner failed: {error}");
            1
        }
    }
}

fn protect_policy_file(path: &Path) -> Result<(), String> {
    platform::protect_policy_file(path)
}

fn launch_restricted(policy: &WindowsPolicy) -> Result<u32, String> {
    platform::launch_restricted(policy)
}

mod platform;

pub fn cleanup_acl_journal() -> Result<(), String> {
    acl::cleanup_journal()?;
    appcontainer::delete_profile()
}

pub(super) async fn run_fs_operation(
    config: &SandboxConfig,
    operation: FsOperation,
) -> std::io::Result<()> {
    fs_impersonation::run(config, operation).await
}
