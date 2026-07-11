use crate::command::CommandRegistry;
use crate::router::Router;
use zode_app_server_protocol::{JsonRpcRequest, RequestId};

#[test]
fn command_exec_captures_output() {
    let mut registry = CommandRegistry;
    let result = registry
        .exec_for_test(vec!["sh".into(), "-c".into(), "printf hi".into()])
        .unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "hi");
}

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
fn router_command_exec_captures_output() {
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);

    let response = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "command/exec".to_string(),
            Some(serde_json::json!({
                "command": ["sh", "-c", "printf hi"]
            })),
        ))
        .unwrap();

    assert_eq!(response.result["stdout"], "hi");
    assert_eq!(response.result["stderr"], "");
    assert_eq!(response.result["exitCode"], 0);
}
