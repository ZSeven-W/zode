use agent::abort::AbortController;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use zode_app_server_protocol::types::{ApprovalPolicy, Thread};
use zode_app_server_protocol::{ErrorObject, JsonRpcMessage, RequestId};

use crate::accumulator::{TurnEndState, TurnOutcome};
use crate::approval_broker::BrokerMsg;
use crate::session::SessionMsg;
use crate::stdio_server::serve;
use crate::turn_host::{HostFactory, TurnHost};

struct EmptyHostFactory;

impl HostFactory for EmptyHostFactory {
    fn build_host(
        &mut self,
        _policy: ApprovalPolicy,
        _turn_ids: mpsc::UnboundedReceiver<String>,
        _broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Box<dyn TurnHost> {
        Box::new(EmptyHost)
    }
}

struct EmptyHost;

#[async_trait]
impl TurnHost for EmptyHost {
    async fn set_model(&mut self, _thread_id: &str, _model: &str) -> Result<(), ErrorObject> {
        Ok(())
    }

    async fn restore_model(&mut self, _thread_id: &str) {}

    async fn start_turn(
        &mut self,
        _thread: &Thread,
        _input: String,
        _model_override: Option<String>,
        _abort: AbortController,
        _msgs: mpsc::Sender<SessionMsg>,
    ) {
    }
}

struct HangingHostFactory;

impl HostFactory for HangingHostFactory {
    fn build_host(
        &mut self,
        _policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
        _broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Box<dyn TurnHost> {
        Box::new(HangingHost { turn_ids })
    }
}

struct HangingHost {
    turn_ids: mpsc::UnboundedReceiver<String>,
}

#[async_trait]
impl TurnHost for HangingHost {
    async fn set_model(&mut self, _thread_id: &str, _model: &str) -> Result<(), ErrorObject> {
        Ok(())
    }

    async fn restore_model(&mut self, _thread_id: &str) {}

    async fn start_turn(
        &mut self,
        thread: &Thread,
        _input: String,
        _model_override: Option<String>,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    ) {
        let Some(turn_id) = self.turn_ids.recv().await else {
            return;
        };
        let thread_id = thread.id.clone();
        tokio::spawn(async move {
            while !abort.is_aborted() {
                tokio::task::yield_now().await;
            }
            let _ = msgs
                .send(SessionMsg::TurnFinished {
                    thread_id,
                    turn_id,
                    outcome: TurnOutcome {
                        state: TurnEndState::Interrupted,
                        final_text: String::new(),
                        usage: Default::default(),
                    },
                })
                .await;
        });
    }
}

async fn run_frames(input: &str, factory: Box<dyn HostFactory>) -> Vec<JsonRpcMessage> {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (server_read, server_write) = tokio::io::split(server);
    let task = tokio::spawn(serve(
        server_read,
        server_write,
        factory,
        "/tmp/zode".into(),
        None,
        60_000,
    ));

    client_write.write_all(input.as_bytes()).await.unwrap();
    client_write.shutdown().await.unwrap();
    let mut output = String::new();
    client_read.read_to_string(&mut output).await.unwrap();
    task.await.unwrap().unwrap();

    output
        .lines()
        .map(|line| zode_app_server_transport::stdio::decode_line(line).unwrap())
        .collect()
}

#[tokio::test]
async fn serve_initialize_then_invalid_frame_then_eof() {
    let messages = run_frames(
        concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test","version":"0"},"approvalPolicy":"auto"}}"#,
            "\nnot-json\n",
        ),
        Box::new(EmptyHostFactory),
    )
    .await;

    assert!(messages.iter().any(|message| matches!(
        message,
        JsonRpcMessage::Response(response)
            if response.result["approvalPolicy"] == "auto"
    )));
    assert!(messages.iter().any(|message| matches!(
        message,
        JsonRpcMessage::Error(error)
            if error.id == RequestId::Null
                && error.error.code == zode_app_server_protocol::rpc::INVALID_REQUEST
    )));
}

#[tokio::test]
async fn eof_with_hanging_turn_emits_interrupted_before_exit() {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (client_read, mut client_write) = tokio::io::split(client);
    let (server_read, server_write) = tokio::io::split(server);
    let task = tokio::spawn(serve(
        server_read,
        server_write,
        Box::new(HangingHostFactory),
        "/tmp/zode".into(),
        None,
        60_000,
    ));
    let mut lines = BufReader::new(client_read).lines();

    client_write
        .write_all(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test","version":"0"},"approvalPolicy":"auto"}}"#,
            "\n",
        ).as_bytes())
        .await
        .unwrap();
    let _initialize = lines.next_line().await.unwrap().unwrap();

    client_write
        .write_all(
            concat!(
                r#"{"jsonrpc":"2.0","id":2,"method":"thread/start","params":{}}"#,
                "\n",
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let thread: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    let thread_id = thread["result"]["thread"]["id"].as_str().unwrap();

    client_write
        .write_all(
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"turn/start\",\"params\":{{\"threadId\":{thread_id:?},\"input\":\"wait\"}}}}\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let _turn_response = lines.next_line().await.unwrap().unwrap();
    let _turn_started = lines.next_line().await.unwrap().unwrap();
    client_write.shutdown().await.unwrap();

    let mut messages = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        messages.push(zode_app_server_transport::stdio::decode_line(&line).unwrap());
    }
    task.await.unwrap().unwrap();

    assert!(messages.iter().any(|message| matches!(
        message,
        JsonRpcMessage::Notification(notification) if notification.method == "turn/interrupted"
    )));
}

async fn start_prompt_command(
    timeout_ms: u64,
) -> (
    tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<std::io::Result<()>>,
    String,
) {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (client_read, mut client_write) = tokio::io::split(client);
    let (server_read, server_write) = tokio::io::split(server);
    let task = tokio::spawn(serve(
        server_read,
        server_write,
        Box::new(EmptyHostFactory),
        "/tmp/zode".into(),
        None,
        timeout_ms,
    ));
    let mut lines = BufReader::new(client_read).lines();

    client_write
        .write_all(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test","version":"0"},"approvalPolicy":"prompt"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"command/exec","params":{"command":["sh","-c","printf hi"]}}"#,
            "\n",
        ).as_bytes())
        .await
        .unwrap();

    let initialize: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(initialize["result"]["approvalPolicy"], "prompt");
    let approval_line = lines.next_line().await.unwrap().unwrap();
    let approval: serde_json::Value = serde_json::from_str(&approval_line).unwrap();
    assert_eq!(approval["method"], "approval/request");
    assert_eq!(approval["params"]["kind"], "command");
    let id = approval["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("srv-"));

    (lines, client_write, task, id)
}

#[tokio::test]
async fn prompt_command_allow_response_executes() {
    let (mut lines, mut write, task, id) = start_prompt_command(60_000).await;
    write
        .write_all(
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id:?},\"result\":{{\"decision\":\"allow\"}}}}\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["stdout"], "hi");
    write.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn prompt_command_deny_response_is_policy_denied() {
    let (mut lines, mut write, task, id) = start_prompt_command(60_000).await;
    write
        .write_all(
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id:?},\"result\":{{\"decision\":\"deny\"}}}}\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(response["id"], 2);
    assert_eq!(
        response["error"]["code"],
        zode_app_server_protocol::rpc::POLICY_DENIED
    );
    write.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn prompt_command_timeout_is_policy_denied() {
    let (mut lines, mut write, task, _id) = start_prompt_command(200).await;
    let response = tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        response["error"]["code"],
        zode_app_server_protocol::rpc::POLICY_DENIED
    );
    write.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn prompt_command_error_frame_is_policy_denied() {
    let (mut lines, mut write, task, id) = start_prompt_command(60_000).await;
    write
        .write_all(
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id:?},\"error\":{{\"code\":-32000,\"message\":\"client refused\"}}}}\n"
            )
                .as_bytes(),
        )
        .await
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        response["error"]["code"],
        zode_app_server_protocol::rpc::POLICY_DENIED
    );
    write.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn eof_with_pending_prompt_approval_exits_cleanly() {
    let (_lines, mut write, task, _id) = start_prompt_command(60_000).await;
    write.shutdown().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .expect("server did not exit after EOF")
        .unwrap()
        .unwrap();
}
