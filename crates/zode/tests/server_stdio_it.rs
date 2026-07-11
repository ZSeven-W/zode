use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// First-frame budget. macOS security assessment (Gatekeeper/XProtect) of a
/// freshly built, ad-hoc-signed debug binary can stall exec in `_dyld_start`
/// for tens of seconds, so the spawn-to-first-response step gets a much wider
/// window than the steady-state steps.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const POLICY_DENIED: i64 = -32010;

struct ServerProcess {
    _config_dir: TempDir,
    _cwd: TempDir,
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
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
            .arg("server")
            .current_dir(cwd.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
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

        let mut child = command.spawn().expect("spawn zode server");
        let stdin = child.stdin.take().expect("child stdin is piped");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout is piped")).lines();
        Self {
            _config_dir: config_dir,
            _cwd: cwd,
            child,
            stdin,
            stdout,
        }
    }

    async fn write(&mut self, frame: Value) {
        let mut encoded = serde_json::to_vec(&frame).expect("serialize JSON-RPC frame");
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .await
            .expect("write JSON-RPC frame");
        self.stdin.flush().await.expect("flush JSON-RPC frame");
    }

    async fn read(&mut self) -> Value {
        self.read_within(STEP_TIMEOUT).await
    }

    /// First read after spawn: covers the exec/loader stall (see
    /// FIRST_FRAME_TIMEOUT) on top of normal request handling.
    async fn read_first(&mut self) -> Value {
        self.read_within(FIRST_FRAME_TIMEOUT).await
    }

    async fn read_within(&mut self, budget: Duration) -> Value {
        let line = tokio::time::timeout(budget, self.stdout.next_line())
            .await
            .expect("server did not produce a frame within the step budget")
            .expect("read server stdout")
            .expect("server stdout closed before the expected frame");
        serde_json::from_str(&line)
            .unwrap_or_else(|err| panic!("invalid JSON frame {line:?}: {err}"))
    }

    async fn close_and_wait(mut self) {
        self.stdin.shutdown().await.expect("close child stdin");
        drop(self.stdin);
        let status = tokio::time::timeout(STEP_TIMEOUT, self.child.wait())
            .await
            .expect("server did not exit within 10 seconds")
            .expect("wait for zode server");
        assert_eq!(status.code(), Some(0), "server must exit cleanly on EOF");
    }
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[tokio::test]
async fn auto_policy_runs_full_stdio_contract() {
    let mut server = ServerProcess::spawn().await;

    server
        .write(request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"stdio-it","version":"0"},
                "approvalPolicy":"auto"
            }),
        ))
        .await;
    let initialize = server.read_first().await;
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "zode");
    assert_eq!(initialize["result"]["approvalPolicy"], "auto");

    server.write(request(2, "thread/start", json!({}))).await;
    let thread = server.read().await;
    assert_eq!(thread["id"], 2);
    let thread_id = thread["result"]["thread"]["id"]
        .as_str()
        .expect("thread/start returns a thread id")
        .to_owned();

    server
        .write(request(
            3,
            "turn/start",
            json!({"threadId":thread_id,"input":"hello"}),
        ))
        .await;
    let turn_response = server.read().await;
    assert_eq!(
        turn_response["id"], 3,
        "turn/start response must precede notifications"
    );
    let turn_id = turn_response["result"]["turn"]["id"]
        .as_str()
        .expect("turn/start returns a turn id")
        .to_owned();

    let started = server.read().await;
    assert_eq!(started["method"], "turn/started");
    assert_eq!(started["params"]["threadId"], thread_id);
    assert_eq!(started["params"]["turnId"], turn_id);

    loop {
        let frame = server.read().await;
        if frame["method"] == "turn/failed" {
            assert_eq!(frame["params"]["threadId"], thread_id);
            assert_eq!(frame["params"]["turnId"], turn_id);
            break;
        }
        assert_ne!(frame["method"], "turn/completed");
        assert_ne!(frame["method"], "turn/interrupted");
    }

    server
        .write(request(4, "command/exec", json!({"command":["echo","hi"]})))
        .await;
    let command = server.read().await;
    assert_eq!(command["id"], 4);
    assert!(command["result"]["stdout"]
        .as_str()
        .expect("command response has stdout")
        .contains("hi"));

    server.close_and_wait().await;
}

#[tokio::test]
async fn default_read_only_policy_denies_command_exec() {
    let mut server = ServerProcess::spawn().await;

    server
        .write(request(
            1,
            "initialize",
            json!({"clientInfo":{"name":"stdio-it","version":"0"}}),
        ))
        .await;
    let initialize = server.read_first().await;
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["approvalPolicy"], "readOnly");

    server
        .write(request(2, "command/exec", json!({"command":["echo","hi"]})))
        .await;
    let denied = server.read().await;
    assert_eq!(denied["id"], 2);
    assert_eq!(denied["error"]["code"], POLICY_DENIED);

    server.close_and_wait().await;
}
