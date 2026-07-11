use crate::command::{exec, OUTPUT_CAP_BYTES};
use crate::router::Router;
use zode_app_server_protocol::types::CommandExecParams;
use zode_app_server_protocol::{JsonRpcRequest, RequestId};

#[tokio::test]
async fn command_exec_captures_output() {
    let result = exec(
        CommandExecParams {
            command: vec!["sh".into(), "-c".into(), "printf hi".into()],
            cwd: None,
            timeout_ms: None,
        },
        None,
    )
    .await
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

#[tokio::test(flavor = "multi_thread")]
async fn router_command_exec_captures_output() {
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

#[tokio::test]
async fn exec_times_out() {
    let params = CommandExecParams {
        command: vec!["sleep".into(), "5".into()],
        cwd: None,
        timeout_ms: Some(200),
    };
    let err = exec(params, None).await.unwrap_err();
    assert!(err.message.contains("timed out"));
}

#[tokio::test]
async fn exec_truncates_output() {
    let params = CommandExecParams {
        command: vec![
            "sh".into(),
            "-c".into(),
            "head -c 2097152 /dev/zero | tr '\\0' 'a'".into(),
        ],
        cwd: None,
        timeout_ms: None,
    };
    let out = exec(params, None).await.unwrap();
    assert!(out.stdout.len() <= OUTPUT_CAP_BYTES + 16);
    assert!(out.stdout.ends_with("[truncated]"));
}
