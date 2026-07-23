//! Optional dynamic install of the `op` CLI via OpenPencil's install-op.sh.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agent::abort::AbortController;
use tokio::process::Command;

use super::{locate, Consent, OpError};
use crate::config::OpenPencilConfig;
use crate::process_supervision::{run_captured, CaptureError, CapturedOutput};

const REPO_RAW: &str = "https://raw.githubusercontent.com/ZSeven-W/openpencil";
const REPO: &str = "https://github.com/ZSeven-W/openpencil";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const INSTALL_OUTPUT_CAP: usize = 64 * 1024;

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
    abort: &AbortController,
) -> Result<PathBuf, OpError> {
    if abort.is_aborted() {
        return Err(aborted(abort));
    }
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
        let confirmed = tokio::select! {
            biased;
            _ = abort.cancelled() => return Err(aborted(abort)),
            confirmed = consent.confirm(&prompt) => confirmed,
        };
        if !confirmed {
            return Err(OpError::InstallDeclined);
        }
    }
    if abort.is_aborted() {
        return Err(aborted(abort));
    }

    // Directory creation and the installer both mutate state. Mark the root
    // turn before either starts so a watchdog never replays a partially
    // completed install after cancellation or timeout.
    abort.mark_side_effect_risk();
    std::fs::create_dir_all(&dir).map_err(|e| OpError::Install(e.to_string()))?;
    let command = if cfg!(windows) {
        let mut command = Command::new("powershell");
        command.arg("-NoProfile").arg("-Command").arg(&cmd);
        command
    } else {
        let mut command = Command::new("bash");
        command.arg("-c").arg(&cmd);
        command
    };
    let out = run_installer(command, abort, INSTALL_TIMEOUT).await?;
    if !out.status.success() {
        let mut tail = String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join("; ");
        if out.stderr_truncated {
            tail.push_str("; stderr truncated");
        }
        return Err(OpError::Install(tail));
    }
    abort.pulse();
    locate::op_command(cfg).ok_or(OpError::NotInstalled)
}

async fn run_installer(
    command: Command,
    abort: &AbortController,
    timeout: Duration,
) -> Result<CapturedOutput, OpError> {
    run_captured(command, abort, timeout, INSTALL_OUTPUT_CAP)
        .await
        .map_err(|error| match error {
            CaptureError::Aborted(reason) => OpError::Aborted(reason),
            CaptureError::TimedOut => OpError::Install(format!(
                "installer timed out after {} seconds",
                timeout.as_secs_f64()
            )),
            CaptureError::Io(error) => OpError::Install(error.to_string()),
            CaptureError::Worker(error) => OpError::Install(error),
        })
}

fn aborted(abort: &AbortController) -> OpError {
    OpError::Aborted(abort.reason().unwrap_or_else(|| "aborted".to_string()))
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
        let abort = AbortController::new();
        assert_eq!(
            ensure_op(&cfg, &YesConsent, "0.8.0", &abort).await.unwrap(),
            op
        );
        assert!(!abort.activity().side_effect_risk());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installer_is_abortable_and_tracked() {
        let abort = AbortController::new();
        let activity = abort.activity();
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("while :; do sleep 1; done");
        let cancel = abort.clone();
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            cancel.abort_with_reason("test cancel");
        });

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run_installer(command, &abort, Duration::from_secs(30)),
        )
        .await
        .expect("installer cancellation should be bounded");
        canceller.await.unwrap();
        assert!(matches!(result, Err(OpError::Aborted(reason)) if reason == "test cancel"));
        tokio::time::timeout(Duration::from_secs(2), activity.wait_for_quiescence())
            .await
            .expect("process supervisor should reap before quiescence");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installer_timeout_is_bounded() {
        let abort = AbortController::new();
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("while :; do sleep 1; done");
        let result = run_installer(command, &abort, Duration::from_millis(25)).await;
        assert!(matches!(result, Err(OpError::Install(message)) if message.contains("timed out")));
        tokio::time::timeout(
            Duration::from_secs(2),
            abort.activity().wait_for_quiescence(),
        )
        .await
        .expect("timed-out installer should be fully reaped");
    }
}
