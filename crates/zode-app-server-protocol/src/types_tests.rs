use super::methods::ClientRequest;
use super::rpc::RequestId;
use super::types::{
    ApprovalPolicy, ClientInfo, CommandExecParams, ConfigWriteParams, ConfigWriteResponse,
    InitializeParams, ModelSetParams, ThreadStartParams, TurnStartParams,
};
use super::{notify, schema};
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
            approval_policy: ApprovalPolicy::ReadOnly,
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id":"init",
            "method":"initialize",
            "params":{
                "clientInfo":{"name":"zode-sdk-test","version":"0.0.0"},
                "approvalPolicy":"readOnly"
            }
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
            model: None,
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
            timeout_ms: None,
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

#[test]
fn initialize_params_default_policy_is_read_only() {
    let p: InitializeParams =
        serde_json::from_str(r#"{"clientInfo":{"name":"t","version":"0"}}"#).unwrap();
    assert_eq!(p.approval_policy, ApprovalPolicy::ReadOnly);
}

#[test]
fn approval_policy_wire_names_are_camel_case() {
    assert_eq!(
        serde_json::to_value(ApprovalPolicy::ReadOnly).unwrap(),
        "readOnly"
    );
    assert_eq!(serde_json::to_value(ApprovalPolicy::Auto).unwrap(), "auto");
    assert_eq!(
        serde_json::to_value(ApprovalPolicy::Prompt).unwrap(),
        "prompt"
    );
}

#[test]
fn turn_notifications_carry_ids() {
    let n = notify::turn_completed("t1", "u1", "hi", &notify::TurnUsage::default());
    assert_eq!(n.method, "turn/completed");
    let p = n.params.unwrap();
    assert_eq!(p["threadId"], "t1");
    assert_eq!(p["turnId"], "u1");
    assert_eq!(p["finalText"], "hi");
}

#[test]
fn turn_interrupt_method_exists() {
    assert!(schema::supported_methods().contains(&"turn/interrupt"));
}

#[test]
fn model_set_params_use_camel_case_thread_id() {
    let params = ModelSetParams {
        thread_id: "thread-1".to_string(),
        model: "gpt-5".to_string(),
    };

    assert_eq!(
        serde_json::to_value(params).unwrap(),
        json!({"threadId": "thread-1", "model": "gpt-5"})
    );
}

#[test]
fn supported_methods_include_model_set_only_as_the_new_client_method() {
    let methods = schema::supported_methods();
    assert_eq!(methods.len(), 27);
    assert!(methods.contains(&"model/set"));
    assert!(methods.contains(&"config/write"));
    assert!(!methods.contains(&"approval/request"));
}

#[test]
fn config_write_uses_camel_case_response_and_defaults_persist() {
    let params: ConfigWriteParams =
        serde_json::from_value(json!({"patch": {"theme": "dark"}})).unwrap();
    assert!(!params.persist);
    assert_eq!(
        serde_json::to_value(ConfigWriteResponse {
            applies_to: "newEngines".into(),
        })
        .unwrap(),
        json!({"appliesTo": "newEngines"})
    );
}

#[test]
fn config_write_request_uses_config_write_method() {
    let req = ClientRequest::ConfigWrite {
        id: RequestId::Number(4),
        params: ConfigWriteParams {
            patch: json!({"theme": "dark"}),
            persist: true,
        },
    };
    assert_eq!(
        serde_json::to_value(req).unwrap(),
        json!({
            "id": 4,
            "method": "config/write",
            "params": {"patch": {"theme": "dark"}, "persist": true}
        })
    );
}
