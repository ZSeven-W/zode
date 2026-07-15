use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde_json::{Map, Value};
use zode_app_server_protocol::rpc::{ErrorObject, INVALID_PARAMS};
use zode_core::config::ZodeConfig;

use crate::error::error;

pub const CONFIG_KEYS: &[&str] = &[
    "provider",
    "providers",
    "images",
    "theme",
    "currency",
    "language",
    "goal",
    "autoLoopMaxTurns",
    "effort",
    "showThinking",
    "showToolDetails",
    "mouseCapture",
    "autonomousOrchestration",
    "skillDiscipline",
    "openspecAwareness",
    "permissions",
    "sandbox",
    "maxOutputTokens",
    "maxIterations",
    "subagentMaxIterations",
    "maxApiRetries",
    "autoUpdate",
    "contextWindow",
    "temperature",
    "promptCache",
    "plugins",
    "tools",
    "lsp",
    "openpencil",
    "browser",
    "noema",
    "compact",
];

pub fn merge_patch(current: &ZodeConfig, patch: Value) -> Result<ZodeConfig, ErrorObject> {
    let patch = patch
        .as_object()
        .ok_or_else(|| error(INVALID_PARAMS, "config patch must be an object"))?;
    for key in patch.keys() {
        if !CONFIG_KEYS.contains(&key.as_str()) {
            return Err(error(INVALID_PARAMS, format!("unknown config key: {key}")));
        }
    }

    let mut merged = serde_json::to_value(current)
        .map_err(|err| error(INVALID_PARAMS, format!("could not serialize config: {err}")))?;
    let object = merged
        .as_object_mut()
        .expect("ZodeConfig always serializes as an object");
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }
    serde_json::from_value(merged)
        .map_err(|err| error(INVALID_PARAMS, format!("invalid config: {err}")))
}

pub fn persist_patch(config_dir: &Path, patch: &Value) -> io::Result<()> {
    let patch = patch.as_object().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "config patch must be an object",
        )
    })?;
    fs::create_dir_all(config_dir)?;
    let path = config_dir.join("config.json");
    let mut document = match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str::<Value>(&raw).map_err(io::Error::other)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(err) => return Err(err),
    };
    let object = document.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "config.json must contain an object",
        )
    })?;
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }

    let mut temp = tempfile::Builder::new()
        .prefix(".config.json.")
        .tempfile_in(config_dir)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), &document).map_err(io::Error::other)?;
    temp.as_file_mut().write_all(b"\n")?;
    temp.as_file_mut().sync_all()?;
    temp.persist(&path).map_err(|err| err.error)?;
    Ok(())
}
