//! Runtime configuration used to build a policy-specific turn host.

use std::path::PathBuf;

use tokio::sync::mpsc;
use zode_app_server_protocol::types::ApprovalPolicy;
use zode_core::config::ZodeConfig;
use zode_core::sandbox::SandboxConfig;

use crate::approval_broker::BrokerMsg;
use crate::turn_host::{EngineHost, HostFactory, TurnHost};

#[derive(Debug, Clone)]
pub struct ServerRuntimeOptions {
    pub cfg: ZodeConfig,
    pub cwd: PathBuf,
    pub sandbox: Option<SandboxConfig>,
    pub date: String,
    pub zode_home: String,
    pub approval_timeout_ms: u64,
    pub dispatch_join_timeout_ms: u64,
}

impl Default for ServerRuntimeOptions {
    fn default() -> Self {
        Self {
            cfg: ZodeConfig::default(),
            cwd: PathBuf::default(),
            sandbox: None,
            date: String::new(),
            zode_home: String::new(),
            approval_timeout_ms: 60_000,
            dispatch_join_timeout_ms: 5_000,
        }
    }
}

impl ServerRuntimeOptions {
    pub fn build_host(
        &self,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
        broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Box<dyn TurnHost> {
        Box::new(EngineHost::new(
            self.cfg.clone(),
            self.cwd.clone(),
            self.sandbox.clone(),
            self.date.clone(),
            policy,
            turn_ids,
            broker,
        ))
    }
}

impl HostFactory for ServerRuntimeOptions {
    fn base_config(&self) -> ZodeConfig {
        self.cfg.clone()
    }

    fn build_host(
        &mut self,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
        broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Box<dyn TurnHost> {
        ServerRuntimeOptions::build_host(self, policy, turn_ids, broker)
    }
}
