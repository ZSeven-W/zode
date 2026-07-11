use zode_app_server_protocol::{JsonRpcMessage, JsonRpcRequest, RequestId};

#[test]
fn decodes_jsonl_message() {
    let msg =
        super::stdio::decode_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Request(JsonRpcRequest::new(
            RequestId::Number(1),
            "initialize".to_string(),
            None
        ))
    );
}

#[test]
fn encodes_message_with_newline() {
    let msg = JsonRpcMessage::Request(JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        None,
    ));
    assert_eq!(
        super::stdio::encode_message(&msg).unwrap(),
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n"
    );
}
