use crate::router::Router;
use zode_app_server_protocol::{JsonRpcRequest, RequestId};

fn init(router: &mut Router) {
    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(0),
            "initialize".to_string(),
            Some(serde_json::json!({
                "clientInfo": {"name": "test", "version": "0.0.0"}
            })),
        ))
        .unwrap();
}

#[test]
fn turn_start_rejects_missing_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let err = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "turn/start".to_string(),
            Some(serde_json::json!({"threadId":"missing","input":"hi"})),
        ))
        .unwrap_err();
    assert!(err.error.message.contains("thread not found"));
}

#[test]
fn turn_start_returns_turn_for_existing_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let thread = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "thread/start".to_string(),
            Some(serde_json::json!({"cwd":"/tmp/project","model":"m"})),
        ))
        .unwrap();
    let thread_id = thread.result["thread"]["id"].as_str().unwrap();

    let turn = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            "turn/start".to_string(),
            Some(serde_json::json!({"threadId":thread_id,"input":"hi"})),
        ))
        .unwrap();

    assert_eq!(turn.result["turn"]["threadId"], thread_id);
    assert_eq!(turn.result["turn"]["status"], "running");
}
