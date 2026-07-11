use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::oneshot;
use zode_app_server_protocol::server_requests::{ApprovalDecision, ApprovalKind};
use zode_app_server_protocol::{JsonRpcMessage, RequestId};
use zode_core::approval::{approval_queue, Approval};

use crate::approval_broker::{ApprovalBroker, BrokerMsg};
use crate::outbound::outbound;

async fn engine_request(
    broker: &tokio::sync::mpsc::Sender<BrokerMsg>,
    tool: &str,
    input: Value,
) -> tokio::task::JoinHandle<Approval> {
    let (queue, mut receiver) = approval_queue();
    let request = tokio::spawn({
        let tool = tool.to_owned();
        async move {
            queue
                .request(&tool, &input, Some("opaque-source".into()))
                .await
        }
    });
    broker
        .send(BrokerMsg::Engine(receiver.next().await.unwrap()))
        .await
        .unwrap();
    request
}

async fn next_request(
    outbound: &mut tokio::sync::mpsc::Receiver<JsonRpcMessage>,
) -> (String, Value) {
    let JsonRpcMessage::Request(request) = outbound.recv().await.unwrap() else {
        panic!("expected approval request")
    };
    let RequestId::String(id) = request.id else {
        panic!("expected string request id")
    };
    assert_eq!(request.method, "approval/request");
    (id, request.params.unwrap())
}

#[tokio::test]
async fn engine_allow_responds_once_and_sends_tool_params() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 5_000);
    let approval = engine_request(&broker, "Bash", json!({"command": "pwd"})).await;

    let (id, params) = next_request(&mut outbound_rx).await;
    assert_eq!(id, "srv-1");
    assert_eq!(params["approvalId"], "srv-1");
    assert_eq!(params["kind"], "tool");
    assert_eq!(params["summary"], "$ pwd");
    assert_eq!(params["tool"], "Bash");
    assert_eq!(params["input"], json!({"command": "pwd"}));
    assert!(params.get("threadId").is_none());
    assert!(params.get("turnId").is_none());

    broker
        .send(BrokerMsg::ClientResponse {
            id,
            decision: Some(ApprovalDecision::Allow),
        })
        .await
        .unwrap();
    assert_eq!(approval.await.unwrap(), Approval::AllowOnce);
    broker.send(BrokerMsg::Shutdown).await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn direct_request_times_out_to_deny() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 100);
    let (reply, decision) = oneshot::channel();
    broker
        .send(BrokerMsg::Direct {
            kind: ApprovalKind::FsWrite,
            summary: "Write settings".into(),
            reply,
        })
        .await
        .unwrap();

    let (_, params) = next_request(&mut outbound_rx).await;
    assert_eq!(params["kind"], "fsWrite");
    assert!(!decision.await.unwrap());
    broker.send(BrokerMsg::Shutdown).await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn engine_allow_always_remembers_tool_without_another_frame() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 5_000);
    let first = engine_request(&broker, "Bash", json!({"command": "pwd"})).await;
    let (id, _) = next_request(&mut outbound_rx).await;
    broker
        .send(BrokerMsg::ClientResponse {
            id,
            decision: Some(ApprovalDecision::AllowAlways),
        })
        .await
        .unwrap();
    assert_eq!(first.await.unwrap(), Approval::AllowAlways);

    let second = engine_request(&broker, "Bash", json!({"command": "whoami"})).await;
    assert_eq!(second.await.unwrap(), Approval::AllowAlways);
    assert!(outbound_rx.try_recv().is_err());
    broker.send(BrokerMsg::Shutdown).await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn shutdown_denies_all_pending_requests() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 5_000);
    let engine = engine_request(&broker, "Bash", json!({"command": "pwd"})).await;
    let _ = next_request(&mut outbound_rx).await;
    let (reply, direct) = oneshot::channel();
    broker
        .send(BrokerMsg::Direct {
            kind: ApprovalKind::Command,
            summary: "Run command".into(),
            reply,
        })
        .await
        .unwrap();
    let _ = next_request(&mut outbound_rx).await;

    broker.send(BrokerMsg::Shutdown).await.unwrap();
    task.await.unwrap();
    assert_eq!(engine.await.unwrap(), Approval::Deny);
    assert!(!direct.await.unwrap());
}

#[tokio::test]
async fn late_and_unknown_responses_are_ignored() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 20);
    let (reply, decision) = oneshot::channel();
    broker
        .send(BrokerMsg::Direct {
            kind: ApprovalKind::Command,
            summary: "Run command".into(),
            reply,
        })
        .await
        .unwrap();
    let (id, _) = next_request(&mut outbound_rx).await;
    assert!(!decision.await.unwrap());

    broker
        .send(BrokerMsg::ClientResponse {
            id,
            decision: Some(ApprovalDecision::Allow),
        })
        .await
        .unwrap();
    broker
        .send(BrokerMsg::ClientResponse {
            id: "srv-unknown".into(),
            decision: None,
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!task.is_finished());
    broker.send(BrokerMsg::Shutdown).await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn dropping_all_senders_denies_pending_and_exits() {
    let (outbound, mut outbound_rx) = outbound();
    let (broker, task) = ApprovalBroker::spawn(outbound, 5_000);
    let engine = engine_request(&broker, "Bash", json!({"command": "pwd"})).await;
    let _ = next_request(&mut outbound_rx).await;

    drop(broker);
    tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("broker retained its own sender")
        .unwrap();
    assert_eq!(engine.await.unwrap(), Approval::Deny);
}
