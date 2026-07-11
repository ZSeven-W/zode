use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::runtime::ServerRuntimeOptions;
use crate::server_file::ServerFile;
use crate::ws_server::{constant_time_eq, run_ws, WsServerConfig};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct TestServer {
    _home: TempDir,
    _cwd: TempDir,
    addr: SocketAddr,
    token: String,
    path: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
}

impl TestServer {
    async fn start(config: WsServerConfig) -> Option<Self> {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let options = ServerRuntimeOptions {
            cwd: cwd.path().to_path_buf(),
            zode_home: home.path().display().to_string(),
            ..ServerRuntimeOptions::default()
        };
        let (ready_tx, ready_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(run_ws(
            options,
            config,
            async move {
                let _ = shutdown_rx.await;
            },
            ready_tx,
        ));
        let (addr, path) = match ready_rx.await {
            Ok(ready) => ready,
            Err(error) => match task.await {
                Ok(Err(server_error))
                    if server_error.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    return None;
                }
                result => panic!("readiness channel closed: {error}; server result: {result:?}"),
            },
        };
        let file: ServerFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        Some(Self {
            _home: home,
            _cwd: cwd,
            addr,
            token: file.token,
            path,
            shutdown: Some(shutdown_tx),
            task,
        })
    }

    async fn connect(&self) -> ClientSocket {
        connect(self.addr, Some(&self.token)).await.unwrap()
    }

    async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        tokio::time::timeout(STEP_TIMEOUT, self.task)
            .await
            .expect("WebSocket server did not stop")
            .unwrap()
            .unwrap();
        assert!(!self.path.exists());
    }
}

fn loopback_config() -> WsServerConfig {
    WsServerConfig {
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        ..WsServerConfig::default()
    }
}

async fn connect(addr: SocketAddr, token: Option<&str>) -> Result<ClientSocket, Error> {
    let mut request = format!("ws://{addr}").into_client_request().unwrap();
    if let Some(token) = token {
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

async fn send(socket: &mut ClientSocket, frame: Value) {
    socket.send(Message::Text(frame.to_string())).await.unwrap();
}

async fn recv(socket: &mut ClientSocket) -> Value {
    let frame = tokio::time::timeout(STEP_TIMEOUT, socket.next())
        .await
        .expect("server did not produce a frame")
        .expect("socket closed before expected frame")
        .expect("failed to read WebSocket frame");
    let Message::Text(text) = frame else {
        panic!("expected text frame, got {frame:?}");
    };
    serde_json::from_str(&text).unwrap()
}

async fn initialize(socket: &mut ClientSocket) {
    send(
        socket,
        request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"ws-it","version":"0"},
                "approvalPolicy":"auto"
            }),
        ),
    )
    .await;
    let response = recv(socket).await;
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["approvalPolicy"], "auto");
}

fn assert_handshake_status(error: Error, expected: StatusCode) {
    match error {
        Error::Http(response) => assert_eq!(response.status(), expected),
        other => panic!("expected HTTP {expected} handshake rejection, got {other:?}"),
    }
}

#[test]
fn bearer_credentials_require_an_exact_match() {
    assert!(constant_time_eq(b"Bearer secret", b"Bearer secret"));
    assert!(!constant_time_eq(b"Bearer secreu", b"Bearer secret"));
    assert!(!constant_time_eq(b"Bearer secret-extra", b"Bearer secret"));
    assert!(!constant_time_eq(b"secret", b"Bearer secret"));
}

#[tokio::test]
async fn correct_token_runs_initialize_and_command_exec() {
    let Some(server) = TestServer::start(loopback_config()).await else {
        return;
    };
    let mut socket = server.connect().await;
    initialize(&mut socket).await;
    send(
        &mut socket,
        request(2, "command/exec", json!({"command":["echo","hi"]})),
    )
    .await;
    let response = recv(&mut socket).await;
    assert_eq!(response["id"], 2);
    assert!(response["result"]["stdout"]
        .as_str()
        .unwrap()
        .contains("hi"));
    socket.close(None).await.unwrap();
    server.stop().await;
}

#[tokio::test]
async fn missing_and_wrong_tokens_are_rejected_with_unauthorized() {
    let Some(server) = TestServer::start(loopback_config()).await else {
        return;
    };
    assert_handshake_status(
        connect(server.addr, None).await.unwrap_err(),
        StatusCode::UNAUTHORIZED,
    );
    assert_handshake_status(
        connect(server.addr, Some("wrong-token")).await.unwrap_err(),
        StatusCode::UNAUTHORIZED,
    );
    server.stop().await;
}

#[tokio::test]
async fn connections_have_isolated_thread_registries() {
    let Some(server) = TestServer::start(loopback_config()).await else {
        return;
    };
    let mut first = server.connect().await;
    let mut second = server.connect().await;
    initialize(&mut first).await;
    initialize(&mut second).await;
    send(&mut first, request(2, "thread/start", json!({}))).await;
    assert_eq!(recv(&mut first).await["id"], 2);
    send(&mut second, request(2, "thread/list", json!({}))).await;
    let listed = recv(&mut second).await;
    assert_eq!(listed["id"], 2);
    assert!(listed["result"]["threads"].as_array().unwrap().is_empty());
    first.close(None).await.unwrap();
    second.close(None).await.unwrap();
    server.stop().await;
}

#[tokio::test]
async fn connection_cap_rejects_then_releases_a_permit() {
    let config = WsServerConfig {
        max_connections: 1,
        ..loopback_config()
    };
    let Some(server) = TestServer::start(config).await else {
        return;
    };
    let mut first = server.connect().await;
    assert_handshake_status(
        connect(server.addr, Some(&server.token)).await.unwrap_err(),
        StatusCode::SERVICE_UNAVAILABLE,
    );
    first.close(None).await.unwrap();
    tokio::time::timeout(STEP_TIMEOUT, async {
        loop {
            match connect(server.addr, Some(&server.token)).await {
                Ok(mut socket) => {
                    socket.close(None).await.unwrap();
                    break;
                }
                Err(Error::Http(response))
                    if response.status() == StatusCode::SERVICE_UNAVAILABLE =>
                {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("new connection failed after permit release: {error:?}"),
            }
        }
    })
    .await
    .expect("connection permit was not released");
    server.stop().await;
}

#[tokio::test]
async fn binary_frame_returns_invalid_request_with_null_id() {
    let Some(server) = TestServer::start(loopback_config()).await else {
        return;
    };
    let mut socket = server.connect().await;
    socket.send(Message::Binary(vec![1, 2, 3])).await.unwrap();
    let response = recv(&mut socket).await;
    assert!(response["id"].is_null());
    assert_eq!(response["error"]["code"], -32600);
    socket.close(None).await.unwrap();
    server.stop().await;
}

#[tokio::test]
async fn shutdown_closes_active_socket_and_removes_server_file() {
    let Some(mut server) = TestServer::start(loopback_config()).await else {
        return;
    };
    let mut socket = server.connect().await;
    let path = server.path.clone();
    server.shutdown.take().unwrap().send(()).unwrap();
    let frame = tokio::time::timeout(STEP_TIMEOUT, socket.next())
        .await
        .expect("client did not observe shutdown")
        .expect("socket ended without a close frame")
        .expect("socket read failed during shutdown");
    assert!(matches!(frame, Message::Close(_)));
    tokio::time::timeout(STEP_TIMEOUT, server.task)
        .await
        .expect("WebSocket server did not stop")
        .unwrap()
        .unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn shutdown_aborts_a_connection_stuck_in_handshake_after_drain_timeout() {
    let config = WsServerConfig {
        drain_timeout_ms: 50,
        ..loopback_config()
    };
    let Some(mut server) = TestServer::start(config).await else {
        return;
    };
    let _stalled_handshake = TcpStream::connect(server.addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let started = Instant::now();
    server.shutdown.take().unwrap().send(()).unwrap();
    tokio::time::timeout(Duration::from_millis(500), server.task)
        .await
        .expect("server drain exceeded its configured timeout")
        .unwrap()
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!server.path.exists());
}
