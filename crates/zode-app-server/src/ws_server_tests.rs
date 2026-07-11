use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tempfile::tempdir;
use tokio::sync::oneshot;

use crate::runtime::ServerRuntimeOptions;
use crate::ws_server::{constant_time_eq, run_ws, WsServerConfig};

#[test]
fn bearer_credentials_require_an_exact_match() {
    assert!(constant_time_eq(b"Bearer secret", b"Bearer secret"));
    assert!(!constant_time_eq(b"Bearer secreu", b"Bearer secret"));
    assert!(!constant_time_eq(b"Bearer secret-extra", b"Bearer secret"));
    assert!(!constant_time_eq(b"secret", b"Bearer secret"));
}

#[tokio::test]
async fn shutdown_cleans_up_server_file_after_readiness() {
    let dir = tempdir().unwrap();
    let options = ServerRuntimeOptions {
        zode_home: dir.path().display().to_string(),
        ..ServerRuntimeOptions::default()
    };
    let config = WsServerConfig {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        ..WsServerConfig::default()
    };
    let (ready_tx, ready_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(run_ws(
        options,
        config,
        async move {
            let _ = shutdown_rx.await;
        },
        ready_tx,
    ));

    let (addr, path) = match ready_rx.await {
        Ok(ready) => ready,
        Err(error) => match server.await {
            Ok(Err(server_error))
                if server_error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            result => panic!("readiness channel closed: {error}; server result: {result:?}"),
        },
    };
    assert_ne!(addr.port(), 0);
    assert!(path.exists());
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
    assert!(!path.exists());
}
