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
fn read_only_capability_methods_return_arrays_or_objects() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);

    for (id, method, key) in [
        (1, "model/list", "providers"),
        (2, "skills/list", "skills"),
        (3, "hooks/list", "hooks"),
        (4, "mcpServerStatus/list", "servers"),
        (5, "plugin/list", "plugins"),
    ] {
        let response = router
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(id),
                method.to_string(),
                Some(serde_json::json!({})),
            ))
            .unwrap();
        assert!(
            response.result[key].is_array(),
            "{method} should return {key}"
        );
    }

    let config = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(6),
            "config/read".to_string(),
            Some(serde_json::json!({})),
        ))
        .unwrap();
    assert!(config.result["config"].is_object());
}
