use crate::router::Router;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
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
fn thread_start_then_list_returns_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let start = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "thread/start".to_string(),
            Some(serde_json::json!({"cwd":"/tmp/project","model":"m"})),
        ))
        .unwrap();
    let thread_id = start.result["thread"]["id"].as_str().unwrap().to_string();
    let list = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            "thread/list".to_string(),
            Some(serde_json::json!({})),
        ))
        .unwrap();
    assert_eq!(list.result["threads"][0]["id"], thread_id);
}

#[test]
fn thread_read_and_resume_return_existing_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let start = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "thread/start".to_string(),
            Some(serde_json::json!({"cwd":"/tmp/project","model":"m"})),
        ))
        .unwrap();
    let thread_id = start.result["thread"]["id"].as_str().unwrap();

    for method in ["thread/read", "thread/resume"] {
        let response = router
            .handle_request(JsonRpcRequest::new(
                RequestId::String(method.to_string()),
                method.to_string(),
                Some(serde_json::json!({"threadId":thread_id})),
            ))
            .unwrap();
        assert_eq!(response.result["thread"]["id"], thread_id);
    }
}

#[test]
fn thread_name_set_updates_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let start = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "thread/start".to_string(),
            Some(serde_json::json!({})),
        ))
        .unwrap();
    let thread_id = start.result["thread"]["id"].as_str().unwrap();

    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            "thread/name/set".to_string(),
            Some(serde_json::json!({"threadId":thread_id,"name":"renamed"})),
        ))
        .unwrap();
    let read = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            "thread/read".to_string(),
            Some(serde_json::json!({"threadId":thread_id})),
        ))
        .unwrap();
    assert_eq!(read.result["thread"]["name"], "renamed");
}

#[test]
fn thread_delete_removes_thread() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let start = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "thread/start".to_string(),
            Some(serde_json::json!({})),
        ))
        .unwrap();
    let thread_id = start.result["thread"]["id"].as_str().unwrap();

    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            "thread/delete".to_string(),
            Some(serde_json::json!({"threadId":thread_id})),
        ))
        .unwrap();
    let list = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            "thread/list".to_string(),
            Some(serde_json::json!({})),
        ))
        .unwrap();
    assert_eq!(list.result["threads"].as_array().unwrap().len(), 0);
}

#[test]
fn unknown_thread_error_preserves_request_id() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);
    let request_id = RequestId::String("read-missing".to_string());

    let err = router
        .handle_request(JsonRpcRequest::new(
            request_id.clone(),
            "thread/read".to_string(),
            Some(serde_json::json!({"threadId":"missing"})),
        ))
        .unwrap_err();

    assert_eq!(err.id, request_id);
    assert_eq!(err.error.code, INVALID_PARAMS);
    assert!(err.error.message.contains("thread not found"));
}
