//! Locate the `op` binary: explicit config → managed dir → PATH.

use std::path::PathBuf;

use crate::config::{ConfigManager, OpenPencilConfig};

/// Whether `cmd` resolves to a file on `PATH` (mirrors `lsp::install::on_path`).
pub fn on_path(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file())
}

/// The `op` binary file name for this platform (`op.exe` on Windows).
pub fn op_exe_name() -> &'static str {
    if cfg!(windows) {
        "op.exe"
    } else {
        "op"
    }
}

/// `<config-dir>/openpencil/bin` — where we install a managed `op`.
pub fn managed_bin_dir() -> Option<PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|d| d.join("openpencil").join("bin"))
}

/// Resolve the `op` binary to run, or `None`.
///
/// Priority order:
/// 1. Explicit `openpencil.op_path` in config.
/// 2. Managed binary at `<config-dir>/openpencil/bin/op[.exe]`.
/// 3. `op[.exe]` found on `PATH` (returns the bare name for `Command::new`).
pub fn op_command(cfg: &OpenPencilConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.op_path {
        return Some(PathBuf::from(p));
    }
    if let Some(managed) = managed_bin_dir() {
        let bin = managed.join(op_exe_name());
        if bin.is_file() {
            return Some(bin);
        }
    }
    on_path(op_exe_name()).then(|| PathBuf::from(op_exe_name()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OpenPencilConfig;

    #[test]
    fn op_path_config_wins() {
        let cfg = OpenPencilConfig {
            op_path: Some("/explicit/op".into()),
            ..Default::default()
        };
        assert_eq!(
            op_command(&cfg),
            Some(std::path::PathBuf::from("/explicit/op"))
        );
    }

    #[test]
    #[serial_test::serial]
    fn falls_back_to_path_when_no_managed_bin() {
        // Isolate the managed dir so a real ~/.zode/openpencil/bin/op can't leak in.
        let cfgdir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", cfgdir.path());
        let dir = tempfile::tempdir().unwrap();
        let op = dir.path().join(op_exe_name()); // op / op.exe per platform
        std::fs::write(&op, "#!/bin/sh\n").unwrap();
        let prev = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());
        let got = op_command(&OpenPencilConfig::default());
        if let Some(p) = prev {
            std::env::set_var("PATH", p);
        } else {
            std::env::remove_var("PATH");
        }
        std::env::remove_var("ZODE_CONFIG_DIR");
        assert_eq!(got, Some(std::path::PathBuf::from(op_exe_name())));
    }
}
