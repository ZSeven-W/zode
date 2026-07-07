use super::rpc::{
    ErrorObject, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use serde_json::json;

#[test]
fn parses_request_without_jsonrpc_header() {
    let raw = r#"{"id":7,"method":"initialize","params":{"clientInfo":{"name":"sdk","version":"0.0.0"}}}"#;
    let msg: JsonRpcMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::Number(7),
            method: "initialize".to_string(),
            params: Some(json!({"clientInfo":{"name":"sdk","version":"0.0.0"}})),
        })
    );
}

#[test]
fn serializes_response_without_jsonrpc_header() {
    let msg = JsonRpcMessage::Response(JsonRpcResponse {
        id: RequestId::String("init".to_string()),
        result: json!({"serverInfo":{"name":"zode","version":"0.0.0"}}),
    });
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(
        value,
        json!({"id":"init","result":{"serverInfo":{"name":"zode","version":"0.0.0"}}})
    );
}

#[test]
fn serializes_error_without_jsonrpc_header() {
    let msg = JsonRpcMessage::Error(JsonRpcError {
        id: RequestId::Number(1),
        error: ErrorObject {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(json!({"method":"missing"})),
        },
    });
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(
        value,
        json!({"id":1,"error":{"code":-32601,"message":"Method not found","data":{"method":"missing"}}})
    );
}

#[test]
fn parses_notification() {
    let raw = r#"{"method":"initialized"}"#;
    let msg: JsonRpcMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Notification(JsonRpcNotification {
            method: "initialized".to_string(),
            params: None,
        })
    );
}
