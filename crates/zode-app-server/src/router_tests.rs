use crate::router::Router;
use zode_app_server_protocol::{JsonRpcRequest, RequestId};

#[test]
fn request_before_initialize_is_rejected() {
    let mut router = Router::for_tests("/tmp/zode");
    let response = router.handle_request(JsonRpcRequest {
        id: RequestId::Number(1),
        method: "thread/list".to_string(),
        params: Some(serde_json::json!({})),
    });
    assert_eq!(
        response.unwrap_err().error.code,
        zode_app_server_protocol::rpc::NOT_INITIALIZED
    );
}

#[test]
fn initialize_request_succeeds() {
    let mut router = Router::for_tests("/tmp/zode");
    let response = router
        .handle_request(JsonRpcRequest {
            id: RequestId::String("init".to_string()),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "clientInfo": {"name": "test", "version": "0.0.0"}
            })),
        })
        .unwrap();
    assert_eq!(response.id, RequestId::String("init".to_string()));
    assert_eq!(response.result["serverInfo"]["name"], "zode");
}
