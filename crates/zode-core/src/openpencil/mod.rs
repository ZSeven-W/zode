//! OpenPencil control surface (`op-bridge`): drive a live OpenPencil instance
//! from zode. `sh` for lifecycle (locate/install/launch), `http` for ops.

pub mod locate;

use thiserror::Error;

/// Failures across the op-bridge.
#[derive(Debug, Error)]
pub enum OpError {
    #[error("the `op` CLI is not installed")]
    NotInstalled,
    #[error("install declined by user")]
    InstallDeclined,
    #[error("install failed: {0}")]
    Install(String),
    #[error("no live OpenPencil instance and none could be launched: {0}")]
    NoInstance(String),
    #[error("launch declined by user")]
    LaunchDeclined,
    #[error("http error: {0}")]
    Http(String),
    #[error("OpenPencil returned an error: {0}")]
    Rpc(String),
    #[error("could not parse response: {0}")]
    Parse(String),
}
