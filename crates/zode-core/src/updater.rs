//! GitHub-release update support, shared by `zode doctor` (which reports whether
//! a newer release exists) and the background auto-updater (which downloads a
//! new build and swaps the binary in place). Every network call is best-effort
//! with a short timeout, so a offline / rate-limited GitHub never blocks zode.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::CoreError;

/// The GitHub repo releases are published to.
pub const REPO: &str = "ZSeven-W/zode";

const RELEASES_API: &str = "https://api.github.com/repos/ZSeven-W/zode/releases";
const USER_AGENT: &str = concat!("zode/", env!("CARGO_PKG_VERSION"));

/// A resolved release plus this platform's tarball download URL (if the release
/// ships one).
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Tag as published, e.g. `v0.1.0-beta.2`.
    pub tag: String,
    /// Version without a leading `v`, e.g. `0.1.0-beta.2`.
    pub version: String,
    /// Download URL for this platform's `.tar.gz`, when present in the release.
    pub asset_url: Option<String>,
}

/// The platform asset suffix used in release filenames (`arm64-mac`,
/// `x64-linux`, `arm64-windows`, …). `None` on an unsupported platform.
pub fn platform_suffix() -> Option<&'static str> {
    Some(match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "macos") => "x64-mac",
        ("aarch64", "macos") => "arm64-mac",
        ("x86_64", "linux") => "x64-linux",
        ("aarch64", "linux") => "arm64-linux",
        ("x86_64", "windows") => "x64-windows",
        ("aarch64", "windows") => "arm64-windows",
        _ => return None,
    })
}

/// Query the latest release (including pre-releases — betas are pre-releases) and
/// resolve this platform's asset URL.
pub async fn latest_release() -> Result<ReleaseInfo, CoreError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| CoreError::Other(format!("http client: {e}")))?;
    let releases: serde_json::Value = client
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|e| CoreError::Other(format!("fetch releases: {e}")))?
        .error_for_status()
        .map_err(|e| CoreError::Other(format!("releases api: {e}")))?
        .json()
        .await
        .map_err(|e| CoreError::Other(format!("parse releases: {e}")))?;
    let first = releases
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| CoreError::Other("no releases published yet".into()))?;
    let tag = first
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Other("release is missing tag_name".into()))?
        .to_string();
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    Ok(ReleaseInfo {
        asset_url: asset_url_for_platform(first, &version),
        tag,
        version,
    })
}

/// The release archive extension for this platform: Windows ships `.zip`, every
/// other platform `.tar.gz` (matches `scripts/build-release.sh`).
fn asset_ext() -> &'static str {
    if cfg!(windows) {
        ".zip"
    } else {
        ".tar.gz"
    }
}

/// Find the `browser_download_url` of this platform's release archive.
fn asset_url_for_platform(release: &serde_json::Value, version: &str) -> Option<String> {
    let suffix = platform_suffix()?;
    let wanted = format!("zode-{version}-{suffix}{}", asset_ext());
    release
        .get("assets")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|a| a.get("name").and_then(|v| v.as_str()) == Some(wanted.as_str()))
        .and_then(|a| a.get("browser_download_url").and_then(|v| v.as_str()))
        .map(str::to_string)
}

/// True when `latest` is strictly newer than `current`. Understands zode's
/// `MAJOR.MINOR.PATCH[-beta.N]` scheme: a final release outranks the same core
/// version carrying a pre-release tag (so `0.1.0` > `0.1.0-beta.9`), and
/// `beta.N` compares numerically.
pub fn is_newer(latest: &str, current: &str) -> bool {
    version_key(latest) > version_key(current)
}

/// Sort key `(major, minor, patch, release_rank, pre_num)`. A final release gets
/// `release_rank = 1` (above any pre-release's `0`). zode only ships `beta.N`
/// pre-releases, so the pre-release *label* isn't ranked — only its number.
fn version_key(v: &str) -> (u64, u64, u64, u8, u64) {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut nums = core
        .split('.')
        .map(|s| s.trim().parse::<u64>().unwrap_or(0));
    let major = nums.next().unwrap_or(0);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    let (release_rank, pre_num) = match pre {
        None => (1, 0),
        Some(p) => (
            0,
            p.rsplit('.')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        ),
    };
    (major, minor, patch, release_rank, pre_num)
}

/// The whole background self-update: skip on a dev build, query the latest
/// release, and — if it's newer and ships a binary for this platform — download
/// and swap it in for the next launch. Returns `Ok(Some(tag))` when a new build
/// was applied, `Ok(None)` when there was nothing to do, `Err` on a best-effort
/// failure the caller can log and ignore.
pub async fn auto_update_if_available(current: &str) -> Result<Option<String>, CoreError> {
    // Windows can't reliably replace a running .exe from within the process, so
    // the silent auto-swap is Unix-only. `zode doctor` still reports that an
    // update is available on Windows; the user updates manually there.
    if cfg!(windows) {
        return Ok(None);
    }
    let exe = std::env::current_exe().map_err(|e| CoreError::Other(format!("current_exe: {e}")))?;
    if looks_like_dev_build(&exe) {
        return Ok(None);
    }
    let rel = latest_release().await?;
    if !is_newer(&rel.version, current) {
        return Ok(None);
    }
    let Some(url) = rel.asset_url.as_deref() else {
        return Ok(None); // no prebuilt binary for this platform
    };
    download_and_apply(url).await?;
    Ok(Some(rel.tag))
}

/// Heuristic: a binary launched straight from a Cargo build dir must NOT be
/// self-replaced with a release download — that would clobber a local dev build.
/// Installed binaries live outside any `target/{debug,release}` path.
pub fn looks_like_dev_build(exe: &Path) -> bool {
    let mut has_target = false;
    let mut has_profile = false;
    for c in exe.components() {
        let s = c.as_os_str();
        has_target |= s == "target";
        has_profile |= s == "debug" || s == "release";
    }
    has_target && has_profile
}

/// Download the release tarball, extract the `zode` binary, and atomically
/// replace the currently-running executable (effective on next launch). Runs in
/// the host process, NOT the sandbox. Returns an error (best-effort for callers)
/// when the network fails or the install dir isn't writable.
pub async fn download_and_apply(asset_url: &str) -> Result<(), CoreError> {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| CoreError::Other(format!("locate current exe: {e}")))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| CoreError::Other(format!("http client: {e}")))?;
    let bytes = client
        .get(asset_url)
        .send()
        .await
        .map_err(|e| CoreError::Other(format!("download: {e}")))?
        .error_for_status()
        .map_err(|e| CoreError::Other(format!("download status: {e}")))?
        .bytes()
        .await
        .map_err(|e| CoreError::Other(format!("download body: {e}")))?;
    let new_bin = extract_zode_binary(&bytes, &exe)?;
    let result = replace_exe(&exe, &new_bin);
    // Best-effort cleanup of the staged binary if the swap didn't consume it.
    let _ = std::fs::remove_file(&new_bin);
    result
}

/// Extract the `zode` binary from an in-memory `.tar.gz` into a temp file beside
/// `exe` (same dir, so the later rename stays on one filesystem). Unix-only (the
/// auto path skips Windows). The staging dir is ALWAYS removed — on success and
/// on every error branch.
fn extract_zode_binary(tarball: &[u8], exe: &Path) -> Result<PathBuf, CoreError> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let bin_name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("zode");
    let stage = dir.join(format!(".{bin_name}.update.{}", std::process::id()));
    let staged_bin = dir.join(format!(".{bin_name}.new.{}", std::process::id()));
    let outcome = extract_into(tarball, &stage, &staged_bin);
    // Whatever happened, never leave the staging dir behind.
    let _ = std::fs::remove_dir_all(&stage);
    outcome.map(|()| staged_bin)
}

/// Unpack the tarball inside `stage` and move the binary to `staged_bin`.
/// Separated so the caller can clean `stage` on any failure.
fn extract_into(tarball: &[u8], stage: &Path, staged_bin: &Path) -> Result<(), CoreError> {
    std::fs::create_dir_all(stage).map_err(|e| CoreError::Other(format!("stage dir: {e}")))?;
    let tgz = stage.join("zode.tar.gz");
    std::fs::write(&tgz, tarball).map_err(|e| CoreError::Other(format!("write tarball: {e}")))?;
    // Extract ONLY the `zode` member: a hostile archive's other entries (absolute
    // paths, `..`, symlinks) are never written, since tar only unpacks the name
    // we ask for and `zode` cannot traverse out of `-C stage`.
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&tgz)
        .arg("-C")
        .arg(stage)
        .arg("zode")
        .status()
        .map_err(|e| CoreError::Other(format!("run tar: {e}")))?;
    if !status.success() {
        return Err(CoreError::Other("tar extraction failed".into()));
    }
    let candidate = stage.join("zode");
    if !candidate.is_file() {
        return Err(CoreError::Other(
            "archive did not contain a zode binary".into(),
        ));
    }
    std::fs::rename(&candidate, staged_bin)
        .map_err(|e| CoreError::Other(format!("stage binary: {e}")))
}

/// Atomically replace `exe` with `new_bin`. On Unix a rename over a running
/// binary is fine (the running process keeps the old inode; the next launch
/// picks up the new file). On Windows the running image can't be overwritten, so
/// move it aside first, then rename the new build into place.
fn replace_exe(exe: &Path, new_bin: &Path) -> Result<(), CoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new_bin, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CoreError::Other(format!("chmod: {e}")))?;
        std::fs::rename(new_bin, exe).map_err(|e| CoreError::Other(format!("swap binary: {e}")))?;
    }
    #[cfg(windows)]
    {
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| CoreError::Other(format!("move old exe: {e}")))?;
        if let Err(e) = std::fs::rename(new_bin, exe) {
            let _ = std::fs::rename(&old, exe); // roll back
            return Err(CoreError::Other(format!("swap binary: {e}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_orders_betas_and_finals() {
        assert!(is_newer("0.1.0-beta.2", "0.1.0-beta.1"));
        assert!(!is_newer("0.1.0-beta.1", "0.1.0-beta.2"));
        assert!(!is_newer("0.1.0-beta.2", "0.1.0-beta.2"));
        // A final release outranks the same core version's pre-releases.
        assert!(is_newer("0.1.0", "0.1.0-beta.9"));
        assert!(!is_newer("0.1.0-beta.9", "0.1.0"));
        // Core version dominates the pre-release tag.
        assert!(is_newer("0.2.0-beta.1", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        // Leading `v` and whitespace are tolerated.
        assert!(is_newer("v0.1.0-beta.3", " 0.1.0-beta.2 "));
    }

    #[test]
    fn dev_builds_are_detected_installed_ones_are_not() {
        assert!(looks_like_dev_build(Path::new(
            "/Users/x/proj/zode/target/debug/zode"
        )));
        assert!(looks_like_dev_build(Path::new(
            "/home/x/zode/target/release/zode"
        )));
        assert!(!looks_like_dev_build(Path::new("/usr/local/bin/zode")));
        assert!(!looks_like_dev_build(Path::new("/home/x/.local/bin/zode")));
    }

    #[test]
    fn asset_url_matches_platform_filename() {
        let suffix = platform_suffix().expect("supported test platform");
        let name = format!("zode-0.1.0-beta.2-{suffix}{}", asset_ext());
        let release = serde_json::json!({
            "tag_name": "v0.1.0-beta.2",
            "assets": [
                { "name": "other.txt", "browser_download_url": "http://x/other" },
                { "name": name, "browser_download_url": "http://x/zode" },
            ]
        });
        assert_eq!(
            asset_url_for_platform(&release, "0.1.0-beta.2").as_deref(),
            Some("http://x/zode")
        );
    }
}
