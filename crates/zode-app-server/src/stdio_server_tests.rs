use agent::abort::AbortController;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use zode_app_server_protocol::types::{ApprovalPolicy, Thread};
use zode_app_server_protocol::{JsonRpcMessage, RequestId};

use crate::accumulator::{TurnEndState, TurnOutcome};
use crate::session::SessionMsg;
use crate::stdio_server::serve;
use crate::turn_host::{HostFactory, TurnHost};

struct EmptyHostFactory;

impl HostFactory for EmptyHostFactory {
    fn build_host(
        &mut self,
        _policy: ApprovalPolicy,
        _turn_ids: mpsc::UnboundedReceiver<String>,
    ) -> Box<dyn TurnHost> {
        Box::new(EmptyHost)
    }
}

struct EmptyHost;

#[async_trait]
impl TurnHost for EmptyHost {
    async fn start_turn(
        &mut self,
        _thread: &Thread,
        _input: String,
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
    ) -> Box<dyn TurnHost> {
        Box::new(HangingHost { turn_ids })
    }
}

struct HangingHost {
    turn_ids: mpsc::UnboundedReceiver<String>,
}

#[async_trait]
impl TurnHost for HangingHost {
    async fn start_turn(
        &mut self,
        thread: &Thread,
        _input: String,
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
