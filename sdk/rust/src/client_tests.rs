use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};
use zode_app_server_protocol::rpc::{JsonRpcMessage, RequestId};
use zode_app_server_protocol::server_requests::ApprovalDecision;
use zode_app_server_protocol::types::{
    ApprovalPolicy, ClientInfo, CommandExecParams, CommandExecResponse, FsReadFileParams,
    InitializeParams, ThreadResponse, ThreadStartParams, TurnResponse, TurnStartParams,
};

use crate::client::{build_request, parse_frame};
use crate::{ClientOptions, ProtocolMethod, ZodeClient};

#[tokio::test]
async fn client_options_default_to_zode_binary() {
    let client = ZodeClient::new(ClientOptions::default());
    assert_eq!(client.binary(), "zode");
}

#[tokio::test]
async fn client_options_allow_binary_override() {
    let client = ZodeClient::new(ClientOptions {
        binary: "/tmp/zode".to_string(),
    });
    assert_eq!(client.binary(), "/tmp/zode");
}

#[test]
fn protocol_method_enum_exposes_wire_names() {
    assert_eq!(ProtocolMethod::Initialize.as_str(), "initialize");
    assert_eq!(ProtocolMethod::CommandExec.as_str(), "command/exec");
    assert_eq!(ProtocolMethod::TurnInterrupt.as_str(), "turn/interrupt");
    assert_eq!(ProtocolMethod::ModelSet.as_str(), "model/set");
    assert_eq!(ProtocolMethod::ConfigWrite.as_str(), "config/write");
    assert_eq!(
        ProtocolMethod::McpServerStatusList.as_str(),
        "mcpServerStatus/list"
    );
    assert_eq!(ProtocolMethod::ALL.len(), 27);
}

fn fixture_request(name: &str, id: RequestId, params: impl Serialize) -> JsonRpcMessage {
    build_request(id, name, params).unwrap()
}

#[test]
fn jsonrpc_fixtures_match_sdk_frames_semantically() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/jsonrpc");
    let mut paths = fs::read_dir(fixture_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".request.json") && !file_name.ends_with(".response.json") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let expected: Value = serde_json::from_str(&source).unwrap();
        if file_name.ends_with(".request.json") {
            let id: RequestId = serde_json::from_value(expected["id"].clone()).unwrap();
            let frame = match expected["method"].as_str().unwrap() {
                "initialize" => fixture_request(
                    "initialize",
                    id,
                    InitializeParams {
                        client_info: ClientInfo {
                            name: "fixture".into(),
                            version: "0.0.0".into(),
                        },
                        approval_policy: ApprovalPolicy::ReadOnly,
                    },
                ),
                "thread/start" => fixture_request(
                    "thread/start",
                    id,
                    ThreadStartParams {
                        cwd: Some("/tmp/project".into()),
                        model: Some("default".into()),
                    },
                ),
                "fs/readFile" => fixture_request(
                    "fs/readFile",
                    id,
                    FsReadFileParams {
                        path: "/tmp/project/hello.txt".into(),
                    },
                ),
                "command/exec" => fixture_request(
                    "command/exec",
                    id,
                    CommandExecParams {
                        command: vec!["sh".into(), "-c".into(), "printf hi".into()],
                        cwd: None,
                        timeout_ms: None,
                    },
                ),
                method => panic!("uncovered request fixture method {method}"),
            };
            assert_eq!(
                serde_json::to_value(frame).unwrap(),
                expected,
                "{file_name}"
            );
        } else if file_name.ends_with(".response.json") {
            let parsed = parse_frame(&source).unwrap();
            assert!(matches!(parsed, JsonRpcMessage::Response(_)), "{file_name}");
            assert_eq!(
                serde_json::to_value(parsed).unwrap(),
                expected,
                "{file_name}"
            );
        }
    }
}

#[test]
fn request_builder_preserves_null_like_json_values() {
    let frame = fixture_request("test", RequestId::Number(9), json!({"value": null}));
    assert_eq!(
        serde_json::to_value(frame).unwrap(),
        json!({"jsonrpc":"2.0","id":9,"method":"test","params":{"value":null}})
    );
}

#[cfg(unix)]
struct TestDir(std::path::PathBuf);

#[cfg(unix)]
impl TestDir {
    fn new(label: &str) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("zode-sdk-{label}-{nonce}"));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(unix)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn scripted_client(script_body: &str) -> ZodeClient {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Timestamp nonces collide when parallel test threads create scripts in
    // the same clock tick, silently cross-wiring tests to each other's
    // scripts; a process-unique counter cannot collide.
    static SCRIPT_SEQ: AtomicU64 = AtomicU64::new(0);
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        SCRIPT_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(format!("zode-sdk-test-{nonce}.sh"));
    fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).unwrap();
    ZodeClient::new(ClientOptions {
        binary: path.to_string_lossy().into_owned(),
    })
}

#[cfg(unix)]
#[tokio::test]
async fn dispatches_out_of_order_responses_and_notifications() {
    let client = scripted_client(
        r#"
read first
read second
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/started","params":{"turnId":"t1"}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"value":"second"}}'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"value":"first"}}'
"#,
    );
    let client = client.spawn_stdio().await.unwrap();
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&notifications);
    client.on_notification(move |method, params| {
        captured.lock().unwrap().push((method, params));
    });

    let (first, second) = tokio::join!(
        client.request::<_, Value>("first", json!({})),
        client.request::<_, Value>("second", json!({}))
    );
    assert_eq!(first.unwrap(), json!({"value":"first"}));
    assert_eq!(second.unwrap(), json!({"value":"second"}));
    tokio::task::yield_now().await;
    assert_eq!(
        *notifications.lock().unwrap(),
        vec![("turn/started".into(), json!({"turnId":"t1"}))]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn approval_handler_response_is_dispatched_without_blocking_reader() {
    let client = scripted_client(
        r#"
read request
printf '%s\n' '{"jsonrpc":"2.0","id":"approval-1","method":"approval/request","params":{"approvalId":"a1","kind":"command","summary":"run"}}'
read approval
case "$approval" in *'"decision":"allow"'*) result=ok;; *) result=wrong;; esac
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"value\":\"$result\"}}"
"#,
    );
    let client = client.spawn_stdio().await.unwrap();
    client.on_approval_request(|params| {
        assert_eq!(params.approval_id, "a1");
        ApprovalDecision::Allow
    });

    let result: Value = client.request("test", json!({})).await.unwrap();
    assert_eq!(result, json!({"value":"ok"}));
}

#[cfg(unix)]
#[tokio::test]
async fn missing_or_panicking_approval_handler_denies() {
    for panic_handler in [false, true] {
        let client = scripted_client(
            r#"
read request
printf '%s\n' '{"jsonrpc":"2.0","id":"approval-1","method":"approval/request","params":{"approvalId":"a1","kind":"tool","summary":"run"}}'
read approval
case "$approval" in *'"decision":"deny"'*) result=denied;; *) result=wrong;; esac
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"value\":\"$result\"}}"
"#,
        );
        let client = client.spawn_stdio().await.unwrap();
        if panic_handler {
            client.on_approval_request(|_| panic!("handler panic"));
        }
        let result: Value = client.request("test", json!({})).await.unwrap();
        assert_eq!(result, json!({"value":"denied"}));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn zode_bin_end_to_end_lifecycle() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let Ok(binary) = std::env::var("ZODE_BIN") else {
        eprintln!("skipping zode_bin_end_to_end_lifecycle: ZODE_BIN is unset");
        return;
    };
    let config_dir = TestDir::new("e2e-config");
    let cwd = TestDir::new("e2e-cwd");
    fs::write(
        config_dir.path().join("config.json"),
        r#"{"provider":{"type":"anthropic"},"sandbox":{"enabled":false}}"#,
    )
    .unwrap();
    let wrapper = config_dir.path().join("zode-e2e-wrapper.sh");
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nunset ANTHROPIC_API_KEY OPENAI_API_KEY\nexport ZODE_CONFIG_DIR={}\ncd {}\nexec {} \"$@\"\n",
            quote(&config_dir.path().to_string_lossy()),
            quote(&cwd.path().to_string_lossy()),
            quote(&binary),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions).unwrap();

    let client = ZodeClient::new(ClientOptions {
        binary: wrapper.to_string_lossy().into_owned(),
    })
    .spawn_stdio()
    .await
    .unwrap();
    let (notifications_tx, mut notifications_rx) = tokio::sync::mpsc::unbounded_channel();
    client.on_notification(move |method, params| {
        let _ = notifications_tx.send((method, params));
    });
    client
        .initialize(
            "rust-sdk-e2e",
            env!("CARGO_PKG_VERSION"),
            ApprovalPolicy::Auto,
        )
        .await
        .unwrap();
    let thread: ThreadResponse = client
        .request(
            "thread/start",
            ThreadStartParams {
                cwd: Some(cwd.path().to_string_lossy().into_owned()),
                model: None,
            },
        )
        .await
        .unwrap();
    let turn: TurnResponse = client
        .request(
            "turn/start",
            TurnStartParams {
                thread_id: thread.thread.id.clone(),
                input: "hello".into(),
                model: None,
            },
        )
        .await
        .unwrap();
    let mut saw_started = false;
    loop {
        let (method, params) =
            tokio::time::timeout(Duration::from_secs(60), notifications_rx.recv())
                .await
                .expect("notification timeout")
                .expect("notification channel closed");
        if method == "turn/started" {
            saw_started = true;
            assert_eq!(params["turnId"], turn.turn.id);
        }
        if method == "turn/failed" {
            assert_eq!(params["turnId"], turn.turn.id);
            break;
        }
    }
    assert!(saw_started);
    let command: CommandExecResponse = client
        .request(
            "command/exec",
            CommandExecParams {
                command: vec!["echo".into(), "hi".into()],
                cwd: None,
                timeout_ms: None,
            },
        )
        .await
        .unwrap();
    assert!(command.stdout.contains("hi"));
    client.close().await.unwrap();
}
