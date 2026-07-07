use crate::initialize::{handle_initialize, ConnectionState};
use zode_app_server_protocol::types::{ClientInfo, InitializeParams};

#[test]
fn initialize_sets_connection_state() {
    let mut state = ConnectionState::default();
    let response = handle_initialize(
        &mut state,
        InitializeParams {
            client_info: ClientInfo {
                name: "test".to_string(),
                version: "0.0.0".to_string(),
            },
        },
        "/tmp/zode".into(),
    )
    .unwrap();
    assert!(state.initialized);
    assert_eq!(state.client_name.as_deref(), Some("test"));
    assert_eq!(response.server_info.name, "zode");
    assert_eq!(response.zode_home, "/tmp/zode");
    assert_eq!(response.platform_family, std::env::consts::FAMILY);
    assert_eq!(response.platform_os, std::env::consts::OS);
    assert_eq!(
        response.capabilities,
        vec![
            "threads".to_string(),
            "turns".to_string(),
            "fs".to_string(),
            "command".to_string(),
            "models".to_string(),
            "config".to_string(),
            "skills".to_string(),
            "hooks".to_string(),
            "mcp".to_string(),
            "plugins".to_string(),
        ]
    );
}

#[test]
fn initialize_twice_is_rejected() {
    let mut state = ConnectionState {
        initialized: true,
        client_name: Some("first".to_string()),
    };
    let err = handle_initialize(
        &mut state,
        InitializeParams {
            client_info: ClientInfo {
                name: "second".to_string(),
                version: "0.0.0".to_string(),
            },
        },
        "/tmp/zode".into(),
    )
    .unwrap_err();
    assert_eq!(err.code, zode_app_server_protocol::rpc::ALREADY_INITIALIZED);
}
