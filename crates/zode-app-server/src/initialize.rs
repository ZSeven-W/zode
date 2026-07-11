use crate::error::{error, ServerResult};
use zode_app_server_protocol::rpc::ALREADY_INITIALIZED;
use zode_app_server_protocol::schema::CAPABILITIES;
use zode_app_server_protocol::types::{InitializeParams, InitializeResponse, ServerInfo};

#[derive(Debug, Default)]
pub struct ConnectionState {
    pub initialized: bool,
    pub client_name: Option<String>,
}

pub fn handle_initialize(
    state: &mut ConnectionState,
    params: InitializeParams,
    zode_home: String,
) -> ServerResult<InitializeResponse> {
    if state.initialized {
        return Err(error(ALREADY_INITIALIZED, "Already initialized"));
    }
    state.initialized = true;
    state.client_name = Some(params.client_info.name);
    let approval_policy = params.approval_policy;
    Ok(InitializeResponse {
        server_info: ServerInfo {
            name: "zode".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        zode_home,
        platform_family: std::env::consts::FAMILY.to_string(),
        platform_os: std::env::consts::OS.to_string(),
        capabilities: CAPABILITIES
            .iter()
            .map(|capability| capability.to_string())
            .collect(),
        approval_policy,
    })
}
