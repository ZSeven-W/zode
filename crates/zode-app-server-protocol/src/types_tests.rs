use super::methods::ClientRequest;
use super::rpc::RequestId;
use super::types::{
    ClientInfo, CommandExecParams, InitializeParams, ThreadStartParams, TurnStartParams,
};
use serde_json::json;

#[test]
fn initialize_request_uses_codex_method_name() {
    let req = ClientRequest::Initialize {
        id: RequestId::String("init".to_string()),
        params: InitializeParams {
            client_info: ClientInfo {
                name: "zode-sdk-test".to_string(),
                version: "0.0.0".to_string(),
            },
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id":"init",
            "method":"initialize",
            "params":{"clientInfo":{"name":"zode-sdk-test","version":"0.0.0"}}
        })
    );
}

#[test]
fn thread_start_request_uses_thread_start_method() {
    let req = ClientRequest::ThreadStart {
        id: RequestId::Number(1),
        params: ThreadStartParams {
            cwd: Some("/tmp/project".to_string()),
            model: Some("deepseek-v4".to_string()),
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id":1,
            "method":"thread/start",
            "params":{"cwd":"/tmp/project","model":"deepseek-v4"}
        })
    );
}

#[test]
fn turn_start_request_uses_turn_start_method() {
    let req = ClientRequest::TurnStart {
        id: RequestId::Number(2),
        params: TurnStartParams {
            thread_id: "thread-1".to_string(),
            input: "hello".to_string(),
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id":2,
            "method":"turn/start",
            "params":{"threadId":"thread-1","input":"hello"}
        })
    );
}

#[test]
fn command_exec_request_uses_command_exec_method() {
    let req = ClientRequest::CommandExec {
        id: RequestId::Number(3),
        params: CommandExecParams {
            command: vec!["sh".to_string(), "-c".to_string(), "printf hi".to_string()],
            cwd: Some("/tmp".to_string()),
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id":3,
            "method":"command/exec",
            "params":{"command":["sh","-c","printf hi"],"cwd":"/tmp"}
        })
    );
}
