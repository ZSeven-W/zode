use crate::command::{exec, OUTPUT_CAP_BYTES};
use zode_app_server_protocol::types::CommandExecParams;

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
