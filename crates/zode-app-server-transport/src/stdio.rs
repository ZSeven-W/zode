use zode_app_server_protocol::JsonRpcMessage;

pub fn decode_line(line: &str) -> serde_json::Result<JsonRpcMessage> {
    serde_json::from_str(line)
}

pub fn encode_message(message: &JsonRpcMessage) -> serde_json::Result<String> {
    serde_json::to_string(message).map(|mut json| {
        json.push('\n');
        json
    })
}
