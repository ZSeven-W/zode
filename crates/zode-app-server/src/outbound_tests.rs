use crate::outbound::{next_batch, outbound, writer_task};
use serde_json::json;
use zode_app_server_protocol::{notify, JsonRpcMessage, JsonRpcResponse, RequestId};

#[tokio::test]
async fn merges_adjacent_deltas_only() {
    let (tx, mut rx) = outbound();
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
    let batch = next_batch(&mut rx).await.unwrap();
    assert_eq!(batch.len(), 2); // merged delta + response
    let JsonRpcMessage::Notification(first) = &batch[0] else {
        panic!("expected merged delta notification");
    };
    assert_eq!(first.params.as_ref().unwrap()["delta"], "ab");
    assert!(matches!(batch[1], JsonRpcMessage::Response(_)));
    assert!(next_batch(&mut rx).await.is_none());
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

#[tokio::test]
async fn adjacent_deltas_from_different_turns_are_not_merged() {
    let (tx, mut rx) = outbound();
    tx.send(JsonRpcMessage::Notification(notify::agent_message_delta(
        "thread", "turn-1", "first",
    )))
    .await
    .unwrap();
    tx.send(JsonRpcMessage::Notification(notify::agent_message_delta(
        "thread", "turn-2", "second",
    )))
    .await
    .unwrap();
    drop(tx);

    let batch = next_batch(&mut rx).await.unwrap();
    assert_eq!(batch.len(), 2);
    let JsonRpcMessage::Notification(first) = &batch[0] else {
        panic!("expected first delta notification");
    };
    let JsonRpcMessage::Notification(second) = &batch[1] else {
        panic!("expected second delta notification");
    };
    assert_eq!(first.params.as_ref().unwrap()["turnId"], "turn-1");
    assert_eq!(first.params.as_ref().unwrap()["delta"], "first");
    assert_eq!(second.params.as_ref().unwrap()["turnId"], "turn-2");
    assert_eq!(second.params.as_ref().unwrap()["delta"], "second");
}
