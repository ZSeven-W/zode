use std::sync::Arc;

use agent::abort::AbortController;
use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};
use zode_app_server_protocol::types::Thread;
use zode_app_server_protocol::{notify, JsonRpcMessage, JsonRpcRequest, RequestId};

use crate::accumulator::{TurnEndState, TurnOutcome};
use crate::outbound::outbound;
use crate::session::{SessionActor, SessionMsg};
use crate::turn_host::TurnHost;

#[derive(Clone)]
enum ScriptStep {
    Delta(&'static str),
    Finish(TurnEndState),
    Hang,
}

struct ScriptedHost {
    script: Vec<ScriptStep>,
    turn_ids: Arc<Mutex<mpsc::UnboundedReceiver<String>>>,
}

#[async_trait]
impl TurnHost for ScriptedHost {
    async fn start_turn(
        &mut self,
        thread: &Thread,
        _input: String,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    ) {
        let script = self.script.clone();
        let ids = self.turn_ids.clone();
        let thread_id = thread.id.clone();
        tokio::spawn(async move {
            let Some(turn_id) = ids.lock().await.recv().await else {
                return;
            };
            for step in script {
                match step {
                    ScriptStep::Delta(delta) => {
                        let _ = msgs
                            .send(SessionMsg::TurnEvent {
                                notification: notify::agent_message_delta(
                                    &thread_id, &turn_id, delta,
                                ),
                            })
                            .await;
                    }
                    ScriptStep::Finish(state) => {
                        let final_text = if matches!(state, TurnEndState::Completed) {
                            "hello".into()
                        } else {
                            String::new()
                        };
                        let _ = msgs
                            .send(SessionMsg::TurnFinished {
                                thread_id: thread_id.clone(),
                                turn_id: turn_id.clone(),
                                outcome: TurnOutcome {
                                    state,
                                    final_text,
                                    usage: Default::default(),
                                },
                            })
                            .await;
                        return;
                    }
                    ScriptStep::Hang => {
                        while !abort.is_aborted() {
                            tokio::task::yield_now().await;
                        }
                        let _ = msgs
                            .send(SessionMsg::TurnFinished {
                                thread_id: thread_id.clone(),
                                turn_id: turn_id.clone(),
                                outcome: TurnOutcome {
                                    state: TurnEndState::Interrupted,
                                    final_text: String::new(),
                                    usage: Default::default(),
                                },
                            })
                            .await;
                        return;
                    }
                }
            }
        });
    }
}

struct Harness {
    tx: mpsc::Sender<SessionMsg>,
    rx: mpsc::Receiver<JsonRpcMessage>,
    ids: mpsc::UnboundedSender<String>,
    actor: tokio::task::JoinHandle<()>,
}
impl Harness {
    fn new(script: Vec<ScriptStep>) -> Self {
        let (out, rx) = outbound();
        let (ids, ids_rx) = mpsc::unbounded_channel();
        let host = ScriptedHost {
            script,
            turn_ids: Arc::new(Mutex::new(ids_rx)),
        };
        let (tx, actor) = SessionActor::spawn(Box::new(host), out, "/tmp/zode".into(), None);
        Self { tx, rx, ids, actor }
    }
    async fn rpc(&self, id: i64, method: &str, params: serde_json::Value) {
        self.tx
            .send(SessionMsg::Rpc(JsonRpcRequest::new(
                RequestId::Number(id),
                method,
                Some(params),
            )))
            .await
            .unwrap();
    }
    async fn next(&mut self) -> JsonRpcMessage {
        tokio::time::timeout(std::time::Duration::from_secs(2), self.rx.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn response(&mut self) -> serde_json::Value {
        match self.next().await {
            JsonRpcMessage::Response(r) => r.result,
            m => panic!("expected response, got {m:?}"),
        }
    }
    async fn init(&mut self, policy: Option<&str>) {
        let mut p = serde_json::json!({"clientInfo":{"name":"test","version":"0"}});
        if let Some(v) = policy {
            p["approvalPolicy"] = v.into();
        }
        self.rpc(0, "initialize", p).await;
        self.response().await;
    }
    async fn thread(&mut self) -> String {
        self.rpc(1, "thread/start", serde_json::json!({})).await;
        self.response().await["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    }
    async fn started(&mut self) -> String {
        match self.next().await {
            JsonRpcMessage::Notification(n) if n.method == "turn/started" => {
                let id = n.params.unwrap()["turnId"].as_str().unwrap().to_string();
                self.ids.send(id.clone()).unwrap();
                id
            }
            m => panic!("expected turn/started, got {m:?}"),
        }
    }
    async fn shutdown(self) {
        let _ = self.tx.send(SessionMsg::Shutdown).await;
        drop(self.tx);
        self.actor.await.unwrap();
    }
}

#[tokio::test]
async fn turn_start_response_precedes_turn_started() {
    let mut h = Harness::new(vec![
        ScriptStep::Delta("hello"),
        ScriptStep::Finish(TurnEndState::Completed),
    ]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"hi"}),
    )
    .await;
    assert!(matches!(h.next().await, JsonRpcMessage::Response(_)));
    h.started().await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="item/agentMessage/delta")
    );
    assert!(matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="turn/completed"));
    h.shutdown().await;
}

#[tokio::test]
async fn second_turn_on_same_thread_is_rejected_with_turn_active() {
    let mut h = Harness::new(vec![ScriptStep::Hang]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"one"}),
    )
    .await;
    h.response().await;
    let id = h.started().await;
    h.rpc(
        3,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"two"}),
    )
    .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Error(e) if e.error.code==zode_app_server_protocol::rpc::TURN_ACTIVE)
    );
    h.rpc(
        4,
        "turn/interrupt",
        serde_json::json!({"threadId":t,"turnId":id}),
    )
    .await;
    h.response().await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="turn/interrupted")
    );
    h.shutdown().await;
}

#[tokio::test]
async fn interrupt_produces_exactly_one_interrupted_terminal() {
    let mut h = Harness::new(vec![ScriptStep::Hang]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"hi"}),
    )
    .await;
    h.response().await;
    let id = h.started().await;
    h.rpc(
        3,
        "turn/interrupt",
        serde_json::json!({"threadId":t,"turnId":id}),
    )
    .await;
    h.response().await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="turn/interrupted")
    );
    assert!(h.rx.try_recv().is_err());
    h.shutdown().await;
}

#[tokio::test]
async fn thread_delete_waits_for_active_turn() {
    let mut h = Harness::new(vec![ScriptStep::Hang]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"hi"}),
    )
    .await;
    h.response().await;
    h.started().await;
    h.rpc(3, "thread/delete", serde_json::json!({"threadId":t}))
        .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="turn/interrupted")
    );
    assert!(matches!(h.next().await,JsonRpcMessage::Response(r) if r.id==RequestId::Number(3)));
    h.rpc(4, "thread/list", serde_json::json!({})).await;
    assert_eq!(h.response().await["threads"].as_array().unwrap().len(), 0);
    h.shutdown().await;
}

#[tokio::test]
async fn thread_delete_twice_answers_both_requests() {
    let mut h = Harness::new(vec![ScriptStep::Hang]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"hi"}),
    )
    .await;
    h.response().await;
    h.started().await;
    h.rpc(3, "thread/delete", serde_json::json!({"threadId":t}))
        .await;
    h.rpc(4, "thread/delete", serde_json::json!({"threadId":t}))
        .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Notification(n) if n.method=="turn/interrupted")
    );
    let mut answered = std::collections::HashSet::new();
    for _ in 0..2 {
        match h.next().await {
            JsonRpcMessage::Response(r) => {
                answered.insert(r.id);
            }
            m => panic!("expected response, got {m:?}"),
        }
    }
    assert!(
        answered.contains(&RequestId::Number(3)),
        "first thread/delete (id 3) never received a response: {answered:?}"
    );
    assert!(
        answered.contains(&RequestId::Number(4)),
        "second thread/delete (id 4) never received a response: {answered:?}"
    );
    h.rpc(5, "thread/list", serde_json::json!({})).await;
    assert_eq!(h.response().await["threads"].as_array().unwrap().len(), 0);
    h.shutdown().await;
}

#[tokio::test]
async fn prompt_policy_rejected_at_initialize() {
    let mut h = Harness::new(vec![]);
    h.rpc(
        0,
        "initialize",
        serde_json::json!({"clientInfo":{"name":"test","version":"0"},"approvalPolicy":"prompt"}),
    )
    .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Error(e) if e.error.code==zode_app_server_protocol::rpc::INVALID_PARAMS && e.error.message=="approvalPolicy 'prompt' is not supported yet")
    );
    h.shutdown().await;
}

#[tokio::test]
async fn read_only_policy_denies_command_exec() {
    let mut h = Harness::new(vec![]);
    h.init(None).await;
    h.rpc(
        1,
        "command/exec",
        serde_json::json!({"command":["printf","hi"]}),
    )
    .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Error(e) if e.error.code==zode_app_server_protocol::rpc::POLICY_DENIED)
    );
    h.shutdown().await;
}

#[tokio::test]
async fn requests_require_initialize_and_initialize_echoes_policy() {
    let mut h = Harness::new(vec![]);
    h.rpc(1, "thread/list", serde_json::json!({})).await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Error(e) if e.error.code==zode_app_server_protocol::rpc::NOT_INITIALIZED)
    );
    h.rpc(
        2,
        "initialize",
        serde_json::json!({"clientInfo":{"name":"test","version":"0"},"approvalPolicy":"auto"}),
    )
    .await;
    assert_eq!(h.response().await["approvalPolicy"], "auto");
    h.shutdown().await;
}

#[tokio::test]
async fn thread_crud_runs_through_actor() {
    let mut h = Harness::new(vec![]);
    h.init(None).await;
    h.rpc(
        1,
        "thread/start",
        serde_json::json!({"cwd":"/tmp/project","model":"m"}),
    )
    .await;
    let t = h.response().await["thread"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.rpc(
        2,
        "thread/name/set",
        serde_json::json!({"threadId":t,"name":"renamed"}),
    )
    .await;
    h.response().await;
    h.rpc(3, "thread/read", serde_json::json!({"threadId":t}))
        .await;
    assert_eq!(h.response().await["thread"]["name"], "renamed");
    h.rpc(4, "thread/resume", serde_json::json!({"threadId":t}))
        .await;
    assert_eq!(h.response().await["thread"]["id"], t);
    h.rpc(5, "thread/delete", serde_json::json!({"threadId":t}))
        .await;
    h.response().await;
    h.rpc(6, "thread/list", serde_json::json!({})).await;
    assert!(h.response().await["threads"].as_array().unwrap().is_empty());
    h.shutdown().await;
}

#[tokio::test]
async fn model_override_is_rejected_and_finished_interrupt_is_idempotent() {
    let mut h = Harness::new(vec![]);
    h.init(None).await;
    let t = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":t,"input":"hi","model":"other"}),
    )
    .await;
    assert!(
        matches!(h.next().await,JsonRpcMessage::Error(e) if e.error.code==zode_app_server_protocol::rpc::INVALID_PARAMS && e.error.message=="model override lands in S2")
    );
    h.rpc(
        3,
        "turn/interrupt",
        serde_json::json!({"threadId":t,"turnId":"finished"}),
    )
    .await;
    assert_eq!(h.response().await["status"], "finished");
    h.shutdown().await;
}

#[tokio::test]
async fn auto_policy_runs_command_without_blocking_actor() {
    let mut h = Harness::new(vec![]);
    h.init(Some("auto")).await;
    h.rpc(
        1,
        "command/exec",
        serde_json::json!({"command":["sh","-c","printf hi"]}),
    )
    .await;
    h.rpc(2, "thread/list", serde_json::json!({})).await;
    let first = h.next().await;
    let second = h.next().await;
    let mut saw_command = false;
    let mut saw_list = false;
    for message in [first, second] {
        if let JsonRpcMessage::Response(r) = message {
            if r.id == RequestId::Number(1) {
                assert_eq!(r.result["stdout"], "hi");
                saw_command = true;
            }
            if r.id == RequestId::Number(2) {
                assert!(r.result["threads"].is_array());
                saw_list = true;
            }
        }
    }
    assert!(saw_command && saw_list);
    h.shutdown().await;
}

#[tokio::test]
async fn auto_policy_fs_write_and_read_run_through_actor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/hello.txt");
    let mut h = Harness::new(vec![]);
    h.init(Some("auto")).await;
    h.rpc(
        1,
        "fs/writeFile",
        serde_json::json!({"path":path,"dataBase64":"aGVsbG8="}),
    )
    .await;
    h.response().await;
    h.rpc(2, "fs/readFile", serde_json::json!({"path":path}))
        .await;
    assert_eq!(h.response().await["dataBase64"], "aGVsbG8=");
    h.shutdown().await;
}

#[tokio::test]
async fn shutdown_aborts_active_turn_emits_terminal_then_exits() {
    let mut h = Harness::new(vec![ScriptStep::Hang]);
    h.init(None).await;
    let thread_id = h.thread().await;
    h.rpc(
        2,
        "turn/start",
        serde_json::json!({"threadId":thread_id,"input":"hi"}),
    )
    .await;
    h.response().await;
    h.started().await;

    h.tx.send(SessionMsg::Shutdown).await.unwrap();
    assert!(
        matches!(h.next().await, JsonRpcMessage::Notification(n) if n.method == "turn/interrupted")
    );
    drop(h.tx);
    tokio::time::timeout(std::time::Duration::from_secs(2), h.actor)
        .await
        .unwrap()
        .unwrap();
}
