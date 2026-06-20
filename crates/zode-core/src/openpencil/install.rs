//! Optional dynamic install of the `op` CLI via OpenPencil's install-op.sh.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::{locate, Consent, OpError};
use crate::config::OpenPencilConfig;

const REPO_RAW: &str = "https://raw.githubusercontent.com/ZSeven-W/openpencil";
const REPO: &str = "https://github.com/ZSeven-W/openpencil";

/// Platform-aware default installer command. Unix pins BOTH the install-op.sh
/// URL to the tag AND `OP_VERSION` (URL-pin alone still downloads the latest
/// asset). Windows has no shell installer, so we download the `op-cli` zip from
/// the GitHub release and extract `op.exe` via PowerShell. Both honor the
/// managed `dir` and the pinned `tag`.
pub fn default_install_command(tag: &str, dir: &Path) -> String {
    if cfg!(windows) {
        format!(
            "$ErrorActionPreference='Stop'; \
             $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {{'aarch64'}} else {{'x86_64'}}; \
             $u = \"{REPO}/releases/download/v{tag}/op-cli-windows-$arch.zip\"; \
             $z = Join-Path $env:TEMP 'op-cli-{tag}.zip'; \
             Invoke-WebRequest -Uri $u -OutFile $z; \
             New-Item -ItemType Directory -Force -Path '{dir}' | Out-Null; \
             Expand-Archive -Force -Path $z -DestinationPath '{dir}'; \
             Remove-Item $z",
            dir = dir.display()
        )
    } else {
        format!(
            "curl -fsSL {REPO_RAW}/v{tag}/scripts/install-op.sh | \
             INSTALL_DIR=\"{}\" OP_VERSION=\"{tag}\" bash",
            dir.display()
        )
    }
}

/// Ensure `op` is runnable, installing on demand after consent.
pub async fn ensure_op(
    cfg: &OpenPencilConfig,
    consent: &dyn Consent,
    tag: &str,
) -> Result<PathBuf, OpError> {
    if let Some(p) = locate::op_command(cfg) {
        return Ok(p);
    }
    let dir = locate::managed_bin_dir().ok_or_else(|| OpError::Install("no config dir".into()))?;
    // Platform-aware default (Unix: install-op.sh via bash; Windows: PowerShell
    // download+extract), overridable via install_command. Run through the
    // platform shell.
    let cmd = cfg
        .install_command
        .clone()
        .unwrap_or_else(|| default_install_command(tag, &dir));
    if !cfg.auto_install() {
        let prompt = format!("Install the OpenPencil `op` CLI? This runs:\n  {cmd}");
        if !consent.confirm(&prompt).await {
            return Err(OpError::InstallDeclined);
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| OpError::Install(e.to_string()))?;
    let out = if cfg!(windows) {
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(&cmd)
            .output()
            .await
    } else {
        Command::new("bash").arg("-c").arg(&cmd).output().await
    }
    .map_err(|e| OpError::Install(e.to_string()))?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stderr);
        let tail: String = tail.lines().rev().take(3).collect::<Vec<_>>().join("; ");
        return Err(OpError::Install(tail));
    }
    locate::op_command(cfg).ok_or(OpError::NotInstalled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_command_pins_url_and_version() {
        let cmd = default_install_command("0.8.0", Path::new("/tmp/x/bin"));
        if cfg!(windows) {
            assert!(cmd.contains("op-cli-windows-"), "{cmd}");
            assert!(cmd.contains("/releases/download/v0.8.0/"), "{cmd}");
            assert!(cmd.to_lowercase().contains("expand-archive"), "{cmd}");
        } else {
            assert!(cmd.contains("/v0.8.0/scripts/install-op.sh"), "{cmd}");
            assert!(cmd.contains("OP_VERSION=\"0.8.0\""), "{cmd}");
            assert!(cmd.contains("INSTALL_DIR=\"/tmp/x/bin\""), "{cmd}");
        }
    }

    #[derive(Debug)]
    struct YesConsent;
    #[async_trait::async_trait]
    impl Consent for YesConsent {
        async fn confirm(&self, _p: &str) -> bool {
            true
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn ensure_returns_existing_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let op = dir.path().join("op");
        std::fs::write(&op, "x").unwrap();
        let cfg = crate::config::OpenPencilConfig {
            op_path: Some(op.to_string_lossy().into()),
            ..Default::default()
        };
        assert_eq!(ensure_op(&cfg, &YesConsent, "0.8.0").await.unwrap(), op);
    }
}
