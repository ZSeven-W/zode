use std::path::Path;

use crate::args::ServerArgs;

pub async fn run(args: &ServerArgs, _cwd: &Path) -> i32 {
    if args.listen == "off" {
        return 0;
    }
    if args.listen == "stdio://" {
        let zode_home = match zode_core::config::ConfigManager::config_dir() {
            Ok(path) => path.display().to_string(),
            Err(err) => {
                eprintln!("zode server: {err}");
                return 1;
            }
        };
        return match zode_app_server::stdio_server::run_stdio(zode_home).await {
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
