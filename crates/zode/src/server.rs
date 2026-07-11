use std::io;
use std::net::SocketAddr;
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
        let options = match bootstrap(cwd).await {
            Ok(options) => options,
            Err(err) => return report_error(err),
        };
        return match zode_app_server::stdio_server::run_stdio(options).await {
            Ok(()) => 0,
            Err(err) => report_error(err),
        };
    }
    if args.listen.starts_with("ws://") {
        let addr = match parse_ws_addr(&args.listen) {
            Ok(addr) => addr,
            Err(err) => return report_error(err),
        };
        let options = match bootstrap(cwd).await {
            Ok(options) => options,
            Err(err) => return report_error(err),
        };
        let config = zode_app_server::ws_server::WsServerConfig {
            addr,
            ..Default::default()
        };
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(zode_app_server::ws_server::run_ws(
            options,
            config,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
            ready_tx,
        ));
        match ready_rx.await {
            Ok((real_addr, path)) => eprintln!(
                "zode server: listening on ws://{real_addr} (credentials: {})",
                path.display()
            ),
            Err(_) => {
                return match server.await {
                    Ok(Err(err)) => report_error(err),
                    Ok(Ok(())) => report_error("server stopped before readiness"),
                    Err(err) => report_error(err),
                };
            }
        }
        return match server.await {
            Ok(Ok(())) => 0,
            Ok(Err(err)) => report_error(err),
            Err(err) => report_error(err),
        };
    }
    eprintln!("zode server: unsupported --listen {}", args.listen);
    1
}

fn parse_ws_addr(listen: &str) -> io::Result<SocketAddr> {
    let authority = listen.strip_prefix("ws://").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebSocket URL must start with ws://",
        )
    })?;
    let host_allowed = authority.starts_with("127.0.0.1:") || authority.starts_with("[::1]:");
    if !host_allowed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WebSocket listener host must be exactly 127.0.0.1 or [::1]",
        ));
    }
    authority.parse::<SocketAddr>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid WebSocket listener URL: {error}"),
        )
    })
}

async fn bootstrap(cwd: &Path) -> io::Result<ServerRuntimeOptions> {
    let mut cfg = ConfigManager::load(cwd).map_err(io::Error::other)?;
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
    let sandbox = zode_core::sandbox::resolve(
        cwd,
        enabled,
        mode,
        cfg.sandbox.network.unwrap_or(false),
        &roots,
        cfg.sandbox.exclude_slash_tmp.unwrap_or(false),
        cfg.sandbox.exclude_tmpdir_env_var.unwrap_or(false),
    )
    .map_err(io::Error::other)?
    .map(|sandbox| sandbox.with_restrict_reads(cfg.sandbox.restrict_reads.unwrap_or(false)));
    if let Some(sandbox) = &sandbox {
        sandbox.verify().await.map_err(io::Error::other)?;
    }

    let zode_home = ConfigManager::config_dir()
        .map_err(io::Error::other)?
        .display()
        .to_string();
    Ok(ServerRuntimeOptions {
        cfg,
        cwd: cwd.to_path_buf(),
        sandbox,
        date: today_date(),
        zode_home,
        approval_timeout_ms: 60_000,
        dispatch_join_timeout_ms: 5_000,
    })
}

fn report_error(error: impl std::fmt::Display) -> i32 {
    eprintln!("zode server: {error}");
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

    #[test]
    fn parses_numeric_loopback_websocket_urls() {
        assert_eq!(parse_ws_addr("ws://127.0.0.1:0").unwrap().port(), 0);
        assert_eq!(parse_ws_addr("ws://[::1]:4321").unwrap().port(), 4321);
    }

    #[test]
    fn rejects_non_loopback_websocket_urls() {
        for listen in [
            "ws://localhost:1234",
            "ws://0.0.0.0:1234",
            "ws://example.com:1234",
            "ws://127.0.0.2:1234",
        ] {
            assert!(parse_ws_addr(listen).is_err(), "accepted {listen}");
        }
    }

    #[tokio::test]
    async fn rejects_unsupported_listen_url() {
        let args = ServerArgs {
            listen: "http://127.0.0.1:0".to_string(),
        };
        assert_eq!(run(&args, Path::new(".")).await, 1);
    }
}
