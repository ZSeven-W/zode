use std::sync::Arc;
use zode_node_protocol::AgentEndpoint;

fn accept_endpoint(_: Arc<dyn AgentEndpoint>) {}

#[test]
fn endpoint_is_object_safe() {
    let _ = accept_endpoint;
}
