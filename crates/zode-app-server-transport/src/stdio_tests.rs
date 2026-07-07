use zode_app_server_protocol::{JsonRpcMessage, JsonRpcRequest, RequestId};

#[test]
fn decodes_jsonl_message() {
    let msg = super::stdio::decode_line(r#"{"id":1,"method":"initialize"}"#).unwrap();
    assert_eq!(
        msg,
        JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::Number(1),
            method: "initialize".to_string(),
            params: None,
        })
    );
}

#[test]
fn encodes_message_with_newline() {
    let msg = JsonRpcMessage::Request(JsonRpcRequest {
        id: RequestId::Number(1),
        method: "initialize".to_string(),
        params: None,
    });
    assert_eq!(
        super::stdio::encode_message(&msg).unwrap(),
        "{\"id\":1,\"method\":\"initialize\"}\n"
    );
}
