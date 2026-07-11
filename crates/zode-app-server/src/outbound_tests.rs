use crate::outbound::{outbound, writer_task};
use serde_json::json;
use zode_app_server_protocol::{notify, JsonRpcMessage, JsonRpcResponse, RequestId};

#[tokio::test]
async fn merges_adjacent_deltas_only() {
    let (tx, rx) = outbound();
    let mut buf = Vec::new();
    tx.send(JsonRpcMessage::Notification(notify::agent_message_delta(
        "t", "u", "a",
    )))
    .await
    .unwrap();
    tx.send(JsonRpcMessage::Notification(notify::agent_message_delta(
        "t", "u", "b",
    )))
    .await
    .unwrap();
    tx.send(JsonRpcMessage::Response(JsonRpcResponse::new(
        RequestId::Number(1),
        json!({}),
    )))
    .await
    .unwrap();
    drop(tx);
    writer_task(rx, &mut buf).await.unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&buf).unwrap().trim().lines().collect();
    assert_eq!(lines.len(), 2); // merged delta + response
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["params"]["delta"], "ab");
}

#[tokio::test]
async fn frames_never_interleave_order() {
    let (tx, rx) = outbound();
    let mut buf = Vec::new();
    tx.send(JsonRpcMessage::Response(JsonRpcResponse::new(
        RequestId::Number(1),
        json!({}),
    )))
    .await
    .unwrap();
    tx.send(JsonRpcMessage::Notification(notify::turn_started("t", "u")))
        .await
        .unwrap();
    drop(tx);
    writer_task(rx, &mut buf).await.unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&buf).unwrap().trim().lines().collect();
    assert!(lines[0].contains("\"id\":1") && lines[1].contains("turn/started"));
}
