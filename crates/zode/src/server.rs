use std::path::{Path, PathBuf};

use crate::args::ServerArgs;
use zode_app_server::runtime::ServerRuntimeOptions;
use zode_core::config::ConfigManager;
use zode_core::sandbox::SandboxMode;

pub async fn run(args: &ServerArgs, cwd: &Path) -> i32 {
    if args.listen == "off" {
        return 0;
    }
    if args.listen == "stdio://" {
        let mut cfg = match ConfigManager::load(cwd) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("zode server: {err}");
                return 1;
            }
        };
        cfg.resolve_provider_from_map();
        cfg.apply_env_fallbacks();

        let enabled = cfg.sandbox.enabled.unwrap_or(true);
        let mode = cfg
            .sandbox
            .mode
            .as_deref()
            .map(SandboxMode::parse)
            .unwrap_or_default();
        let roots: Vec<PathBuf> = cfg
            .sandbox
            .writable_roots
            .iter()
            .map(PathBuf::from)
            .collect();
        let sandbox = match zode_core::sandbox::resolve(
            cwd,
            enabled,
            mode,
            cfg.sandbox.network.unwrap_or(false),
            &roots,
            cfg.sandbox.exclude_slash_tmp.unwrap_or(false),
            cfg.sandbox.exclude_tmpdir_env_var.unwrap_or(false),
        ) {
            Ok(sandbox) => sandbox.map(|sandbox| {
                sandbox.with_restrict_reads(cfg.sandbox.restrict_reads.unwrap_or(false))
            }),
            Err(err) => {
                eprintln!("zode server: {err}");
                return 1;
            }
        };
        if let Some(sandbox) = &sandbox {
            if let Err(err) = sandbox.verify().await {
                eprintln!("zode server: {err}");
                return 1;
            }
        }

        let zode_home = match ConfigManager::config_dir() {
            Ok(path) => path.display().to_string(),
            Err(err) => {
                eprintln!("zode server: {err}");
                return 1;
            }
        };
        let options = ServerRuntimeOptions {
            cfg,
            cwd: cwd.to_path_buf(),
            sandbox,
            date: today_date(),
            zode_home,
        };
        return match zode_app_server::stdio_server::run_stdio(options).await {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("zode server: {err}");
                1
            }
        };
    }
    eprintln!("zode server: unsupported --listen {}", args.listen);
    1
}

fn today_date() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_off_listen_url() {
        let args = ServerArgs {
            listen: "off".to_string(),
        };
        assert_eq!(run(&args, Path::new(".")).await, 0);
    }

    #[tokio::test]
    async fn rejects_websocket_listen_url_until_ws_runtime_exists() {
        let args = ServerArgs {
            listen: "ws://127.0.0.1:0".to_string(),
        };
        assert_eq!(run(&args, Path::new(".")).await, 1);
    }

    #[tokio::test]
    async fn rejects_unsupported_listen_url() {
        let args = ServerArgs {
            listen: "http://127.0.0.1:0".to_string(),
        };
        assert_eq!(run(&args, Path::new(".")).await, 1);
    }
}
