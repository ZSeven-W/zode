use crate::{ClientOptions, ProtocolMethod, ZodeClient};

#[tokio::test]
async fn client_options_default_to_zode_binary() {
    let client = ZodeClient::new(ClientOptions::default());
    assert_eq!(client.binary(), "zode");
}

#[tokio::test]
async fn client_options_allow_binary_override() {
    let client = ZodeClient::new(ClientOptions {
        binary: "/tmp/zode".to_string(),
    });
    assert_eq!(client.binary(), "/tmp/zode");
}

#[test]
fn protocol_method_enum_exposes_wire_names() {
    assert_eq!(ProtocolMethod::Initialize.as_str(), "initialize");
    assert_eq!(ProtocolMethod::CommandExec.as_str(), "command/exec");
    assert_eq!(ProtocolMethod::TurnInterrupt.as_str(), "turn/interrupt");
    assert_eq!(ProtocolMethod::ModelSet.as_str(), "model/set");
    assert_eq!(ProtocolMethod::ConfigWrite.as_str(), "config/write");
    assert_eq!(
        ProtocolMethod::McpServerStatusList.as_str(),
        "mcpServerStatus/list"
    );
    assert_eq!(ProtocolMethod::ALL.len(), 27);
}
