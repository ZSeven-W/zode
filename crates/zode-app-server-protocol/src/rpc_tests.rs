use super::rpc::{
    ErrorObject, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use serde_json::json;

#[test]
fn parses_request_with_jsonrpc_header() {
    let raw = r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{"clientInfo":{"name":"sdk","version":"0.0.0"}}}"#;
    let msg: JsonRpcMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Request(JsonRpcRequest::new(
            RequestId::Number(7),
            "initialize".to_string(),
            Some(json!({"clientInfo":{"name":"sdk","version":"0.0.0"}}))
        ))
    );
}

#[test]
fn serializes_response_with_jsonrpc_header() {
    let msg = JsonRpcMessage::Response(JsonRpcResponse::new(
        RequestId::String("init".to_string()),
        json!({"serverInfo":{"name":"zode","version":"0.0.0"}}),
    ));
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(
        value,
        json!({"jsonrpc":"2.0","id":"init","result":{"serverInfo":{"name":"zode","version":"0.0.0"}}})
    );
}

#[test]
fn serializes_error_with_jsonrpc_header() {
    let msg = JsonRpcMessage::Error(JsonRpcError::new(
        RequestId::Number(1),
        ErrorObject {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(json!({"method":"missing"})),
        },
    ));
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(
        value,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found","data":{"method":"missing"}}})
    );
}

#[test]
fn parses_notification() {
    let raw = r#"{"jsonrpc":"2.0","method":"initialized"}"#;
    let msg: JsonRpcMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Notification(JsonRpcNotification::new("initialized".to_string(), None))
    );
}

#[test]
fn frames_carry_jsonrpc_2_0() {
    let resp = JsonRpcResponse::new(RequestId::Number(1), serde_json::json!({}));
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
}

#[test]
fn missing_jsonrpc_is_rejected() {
    let raw = r#"{"id":1,"method":"ping"}"#;
    assert!(serde_json::from_str::<JsonRpcMessage>(raw).is_err());
}

#[test]
fn wrong_version_is_rejected() {
    let raw = r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#;
    assert!(serde_json::from_str::<JsonRpcMessage>(raw).is_err());
}

#[test]
fn ambiguous_frame_is_rejected() {
    // id+method+result matches no strict frame shape
    let raw = r#"{"jsonrpc":"2.0","id":1,"method":"x","result":{}}"#;
    assert!(serde_json::from_str::<JsonRpcMessage>(raw).is_err());
}

#[test]
fn null_id_roundtrip() {
    let raw = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"x"}}"#;
    let msg: JsonRpcMessage = serde_json::from_str(raw).unwrap();
    assert!(matches!(msg, JsonRpcMessage::Error(e) if e.id == RequestId::Null));
}
