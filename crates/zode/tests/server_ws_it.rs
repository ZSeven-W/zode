#![cfg(unix)]

use std::process::Stdio;
use std::time::Duration;

use futures::StreamExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStderr, Command};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// macOS security assessment of a freshly built debug binary can stall exec
/// for tens of seconds, so discovery and the first frame get a wider budget.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(60);
type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct ServerProcess {
    config_dir: TempDir,
    _cwd: TempDir,
    child: Child,
    stderr: Option<ChildStderr>,
}

impl ServerProcess {
    async fn spawn() -> Self {
        let config_dir = tempfile::tempdir().expect("create isolated config directory");
        let cwd = tempfile::tempdir().expect("create isolated server cwd");
        std::fs::write(
            config_dir.path().join("config.json"),
            r#"{"provider":{"type":"anthropic"},"sandbox":{"enabled":false}}"#,
        )
        .expect("write isolated config");

        let mut command = Command::new(env!("CARGO_BIN_EXE_zode"));
        command
            .args(["server", "--listen", "ws://127.0.0.1:0"])
            .current_dir(cwd.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("ZODE_") {
                command.env_remove(key);
            }
        }
        command
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env("ZODE_CONFIG_DIR", config_dir.path());

        let mut child = command.spawn().expect("spawn zode WebSocket server");
        let stderr = child.stderr.take();
        Self {
            config_dir,
            _cwd: cwd,
            child,
            stderr,
        }
    }

    async fn wait_for_discovery(&mut self) -> Option<Value> {
        let path = self.config_dir.path().join("server.json");
        tokio::time::timeout(FIRST_FRAME_TIMEOUT, async {
            loop {
                if let Ok(bytes) = std::fs::read(&path) {
                    break Some(serde_json::from_slice(&bytes).expect("parse server.json"));
                }
                if let Some(status) = self.child.try_wait().expect("poll zode server") {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = self.stderr.take() {
                        pipe.read_to_string(&mut stderr).await.unwrap();
                    }
                    if !status.success() && stderr.contains("Operation not permitted") {
                        break None;
                    }
                    panic!("server exited before discovery with {status}: {stderr}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("server.json was not created within 60 seconds")
    }

    async fn interrupt_and_wait(mut self) {
        let pid = self.child.id().expect("server child has a pid");
        let signal = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .expect("send SIGINT to zode server");
        assert!(signal.success(), "kill -INT failed");
        let status = tokio::time::timeout(STEP_TIMEOUT, self.child.wait())
            .await
            .expect("server did not exit within 10 seconds after SIGINT")
            .expect("wait for zode server");
        assert_eq!(status.code(), Some(0), "server must exit cleanly on SIGINT");
        assert!(!self.config_dir.path().join("server.json").exists());
    }
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

async fn send(socket: &mut ClientSocket, frame: Value) {
    use futures::SinkExt;
    socket.send(Message::Text(frame.to_string())).await.unwrap();
}

async fn read(socket: &mut ClientSocket, budget: Duration) -> Value {
    let frame = tokio::time::timeout(budget, socket.next())
        .await
        .expect("server did not produce a frame within the step budget")
        .expect("socket closed before the expected frame")
        .expect("read WebSocket frame");
    let Message::Text(text) = frame else {
        panic!("expected text frame, got {frame:?}");
    };
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("invalid frame {text:?}: {error}"))
}

#[tokio::test]
async fn websocket_binary_contract_and_sigint_cleanup() {
    let mut server = ServerProcess::spawn().await;
    let Some(discovery) = server.wait_for_discovery().await else {
        return;
    };
    let port = discovery["port"].as_u64().expect("server.json port");
    let token = discovery["token"].as_str().expect("server.json token");
    let mut handshake = format!("ws://127.0.0.1:{port}")
        .into_client_request()
        .unwrap();
    handshake.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(handshake)
        .await
        .expect("connect to zode WebSocket server");

    send(
        &mut socket,
        request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"ws-e2e","version":"0"},
                "approvalPolicy":"auto"
            }),
        ),
    )
    .await;
    let initialize = read(&mut socket, FIRST_FRAME_TIMEOUT).await;
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["approvalPolicy"], "auto");

    send(&mut socket, request(2, "thread/start", json!({}))).await;
    let thread = read(&mut socket, STEP_TIMEOUT).await;
    assert_eq!(thread["id"], 2);
    let thread_id = thread["result"]["thread"]["id"]
        .as_str()
        .expect("thread/start returns an id")
        .to_owned();

    send(
        &mut socket,
        request(
            3,
            "turn/start",
            json!({"threadId":thread_id,"input":"hello"}),
        ),
    )
    .await;
    let turn = read(&mut socket, STEP_TIMEOUT).await;
    assert_eq!(turn["id"], 3, "turn response must precede notifications");
    let turn_id = turn["result"]["turn"]["id"].as_str().unwrap().to_owned();
    let started = read(&mut socket, STEP_TIMEOUT).await;
    assert_eq!(started["method"], "turn/started");
    assert_eq!(started["params"]["threadId"], thread_id);
    assert_eq!(started["params"]["turnId"], turn_id);
    loop {
        let frame = read(&mut socket, STEP_TIMEOUT).await;
        if frame["method"] == "turn/failed" {
            assert_eq!(frame["params"]["threadId"], thread_id);
            assert_eq!(frame["params"]["turnId"], turn_id);
            break;
        }
        assert_ne!(frame["method"], "turn/completed");
        assert_ne!(frame["method"], "turn/interrupted");
    }

    send(
        &mut socket,
        request(4, "command/exec", json!({"command":["echo","hi"]})),
    )
    .await;
    let command = read(&mut socket, STEP_TIMEOUT).await;
    assert_eq!(command["id"], 4);
    assert!(command["result"]["stdout"].as_str().unwrap().contains("hi"));

    server.interrupt_and_wait().await;
}
