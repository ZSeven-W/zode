//! Stable installation identity for the local application node.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zode_core::persistence::{write_atomic, AdvisoryFileLock};
use zode_core::CoreError;
use zode_node_protocol::NodeId;

const IDENTITY_VERSION: u64 = 1;

/// Persists one stable node identity beneath an explicit config directory.
#[derive(Debug, Clone)]
pub struct NodeIdentityStore {
    path: PathBuf,
}

impl NodeIdentityStore {
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            path: config_dir.as_ref().join("node.json"),
        }
    }

    /// Loads the existing identity or atomically creates the first one.
    ///
    /// Corrupt and unsupported files are reported without being replaced.
    pub fn load_or_create(&self) -> Result<NodeId, CoreError> {
        let _lock = AdvisoryFileLock::acquire(&self.path)?;
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_identity(&bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let node_id = NodeId::new();
                let bytes = encode_identity(node_id)?;
                write_atomic(&self.path, &bytes)?;
                Ok(node_id)
            }
            Err(error) => Err(CoreError::Io(error)),
        }
    }
}

fn parse_identity(bytes: &[u8]) -> Result<NodeId, CoreError> {
    let value: Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| CoreError::Other("node identity has no valid version".to_string()))?;
    if version != IDENTITY_VERSION {
        return Err(CoreError::Other(format!(
            "unsupported node identity version: {version}"
        )));
    }
    let node_id = value
        .get("nodeId")
        .cloned()
        .ok_or_else(|| CoreError::Other("node identity has no nodeId".to_string()))?;
    serde_json::from_value(node_id).map_err(CoreError::Json)
}

fn encode_identity(node_id: NodeId) -> Result<Vec<u8>, CoreError> {
    let mut object = Map::new();
    object.insert("version".to_string(), Value::from(IDENTITY_VERSION));
    object.insert("nodeId".to_string(), serde_json::to_value(node_id)?);
    serde_json::to_vec(&Value::Object(object)).map_err(CoreError::Json)
}
