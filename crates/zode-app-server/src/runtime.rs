//! Runtime configuration used to build a policy-specific turn host.

use std::path::PathBuf;

use tokio::sync::mpsc;
use zode_app_server_protocol::types::ApprovalPolicy;
use zode_core::config::ZodeConfig;
use zode_core::sandbox::SandboxConfig;

use crate::turn_host::{EngineHost, HostFactory, TurnHost};

#[derive(Debug, Clone)]
pub struct ServerRuntimeOptions {
    pub cfg: ZodeConfig,
    pub cwd: PathBuf,
    pub sandbox: Option<SandboxConfig>,
    pub date: String,
    pub zode_home: String,
}

impl ServerRuntimeOptions {
    pub fn build_host(
        &self,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
    ) -> Box<dyn TurnHost> {
        Box::new(EngineHost::new(
            self.cfg.clone(),
            self.cwd.clone(),
            self.sandbox.clone(),
            self.date.clone(),
            policy,
            turn_ids,
        ))
    }
}

impl HostFactory for ServerRuntimeOptions {
    fn build_host(
        &mut self,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
    ) -> Box<dyn TurnHost> {
        ServerRuntimeOptions::build_host(self, policy, turn_ids)
    }
}
