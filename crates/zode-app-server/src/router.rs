//! Stateless request dispatch used by the session actor.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::de::DeserializeOwned;
use serde_json::Value;
use zode_app_server_protocol::rpc::{ErrorObject, INVALID_PARAMS};
use zode_app_server_protocol::types::*;
use zode_core::sandbox::SandboxConfig;

use crate::capabilities::*;
use crate::error::error;
use crate::fs::{read_file_base64, write_file_base64};
use crate::policy::DirectKind;

pub fn method_kind(method: &str) -> Option<Option<DirectKind>> {
    match method {
        "model/set" => Some(None),
        "command/exec" => Some(Some(DirectKind::Command)),
        "fs/writeFile" | "fs/createDirectory" | "fs/remove" | "fs/copy" => {
            Some(Some(DirectKind::FsWrite))
        }
        "fs/readFile"
        | "fs/getMetadata"
        | "fs/readDirectory"
        | "model/list"
        | "config/read"
        | "config/list"
        | "skills/list"
        | "skills/read"
        | "hooks/list"
        | "mcpServerStatus/list"
        | "plugin/list" => Some(None),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
pub async fn dispatch_stateless(
    method: &str,
    params: Option<Value>,
    sandbox: Option<&SandboxConfig>,
) -> Result<Value, ErrorObject> {
    let value = match method {
        "command/exec" => {
            serde_json::to_value(crate::command::exec(parse_params(params)?, sandbox).await?)
                .unwrap_or(Value::Null)
        }
        "fs/readFile" => {
            let p: FsReadFileParams = parse_params(params)?;
            let data_base64 = read_file_base64(Path::new(&p.path))
                .map_err(|e| error(INVALID_PARAMS, format!("fs/readFile: {e}")))?;
            serde_json::to_value(FsReadFileResponse { data_base64 }).unwrap_or(Value::Null)
        }
        "fs/writeFile" => {
            let p: FsWriteFileParams = parse_params(params)?;
            write_file_base64(Path::new(&p.path), &p.data_base64)
                .await
                .map_err(|e| error(INVALID_PARAMS, format!("fs/writeFile: {e}")))?;
            empty()
        }
        "fs/createDirectory" => {
            let p: FsCreateDirectoryParams = parse_params(params)?;
            let result = if p.recursive.unwrap_or(true) {
                tokio::fs::create_dir_all(&p.path).await
            } else {
                tokio::fs::create_dir(&p.path).await
            };
            result.map_err(|e| error(INVALID_PARAMS, format!("fs/createDirectory: {e}")))?;
            empty()
        }
        "fs/getMetadata" => {
            let p: FsGetMetadataParams = parse_params(params)?;
            let symlink = std::fs::symlink_metadata(&p.path)
                .map_err(|e| error(INVALID_PARAMS, format!("fs/getMetadata: {e}")))?;
            let meta = std::fs::metadata(&p.path).unwrap_or_else(|_| symlink.clone());
            serde_json::to_value(FsGetMetadataResponse {
                is_directory: meta.is_dir(),
                is_file: meta.is_file(),
                is_symlink: symlink.file_type().is_symlink(),
                created_at_ms: system_time_ms(symlink.created().ok()),
                modified_at_ms: system_time_ms(symlink.modified().ok()),
            })
            .unwrap_or(Value::Null)
        }
        "fs/readDirectory" => {
            let p: FsReadDirectoryParams = parse_params(params)?;
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&p.path)
                .map_err(|e| error(INVALID_PARAMS, format!("fs/readDirectory: {e}")))?
            {
                let entry =
                    entry.map_err(|e| error(INVALID_PARAMS, format!("fs/readDirectory: {e}")))?;
                let meta = entry
                    .metadata()
                    .map_err(|e| error(INVALID_PARAMS, format!("fs/readDirectory: {e}")))?;
                entries.push(FsReadDirectoryEntry {
                    file_name: entry.file_name().to_string_lossy().into_owned(),
                    is_directory: meta.is_dir(),
                    is_file: meta.is_file(),
                });
            }
            entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            serde_json::to_value(FsReadDirectoryResponse { entries }).unwrap_or(Value::Null)
        }
        "fs/remove" => {
            let p: FsRemoveParams = parse_params(params)?;
            remove_path(Path::new(&p.path), p.recursive, p.force)
                .await
                .map_err(|e| error(INVALID_PARAMS, format!("fs/remove: {e}")))?;
            empty()
        }
        "fs/copy" => {
            let p: FsCopyParams = parse_params(params)?;
            copy_path(
                Path::new(&p.source_path),
                Path::new(&p.destination_path),
                p.recursive.unwrap_or(false),
            )
            .await
            .map_err(|e| error(INVALID_PARAMS, format!("fs/copy: {e}")))?;
            empty()
        }
        "model/list" => serde_json::to_value(model_list()).unwrap_or(Value::Null),
        "config/read" => serde_json::to_value(config_read()?).unwrap_or(Value::Null),
        "config/list" => serde_json::to_value(config_list()?).unwrap_or(Value::Null),
        "skills/list" => serde_json::to_value(skills_list(&cwd())?).unwrap_or(Value::Null),
        "skills/read" => {
            serde_json::to_value(skills_read(&cwd(), parse_params(params)?)?).unwrap_or(Value::Null)
        }
        "hooks/list" => serde_json::to_value(hooks_list(&cwd())).unwrap_or(Value::Null),
        "mcpServerStatus/list" => {
            serde_json::to_value(mcp_server_status_list(&cwd())).unwrap_or(Value::Null)
        }
        "plugin/list" => serde_json::to_value(plugin_list(&cwd())?).unwrap_or(Value::Null),
        _ => unreachable!("method_kind guards dispatch"),
    };
    Ok(value)
}

pub fn parse_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T, ErrorObject> {
    serde_json::from_value(params.unwrap_or(Value::Object(Default::default())))
        .map_err(|e| error(INVALID_PARAMS, format!("Invalid params: {e}")))
}
fn empty() -> Value {
    serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
}
fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
fn system_time_ms(v: Option<std::time::SystemTime>) -> i64 {
    v.and_then(|v| v.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
async fn remove_path(
    path: &Path,
    recursive: Option<bool>,
    force: Option<bool>,
) -> std::io::Result<()> {
    let meta = match tokio::fs::symlink_metadata(path).await {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && force.unwrap_or(true) => {
            return Ok(())
        }
        Err(e) => return Err(e),
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        if recursive.unwrap_or(true) {
            tokio::fs::remove_dir_all(path).await
        } else {
            tokio::fs::remove_dir(path).await
        }
    } else {
        tokio::fs::remove_file(path).await
    }
}
async fn copy_path(source: &Path, dest: &Path, recursive: bool) -> std::io::Result<()> {
    let meta = tokio::fs::metadata(source).await?;
    if meta.is_dir() {
        if !recursive {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "recursive must be true to copy directories",
            ));
        }
        copy_dir_recursive(source, dest).await
    } else {
        if let Some(p) = dest.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        tokio::fs::copy(source, dest).await.map(|_| ())
    }
}
async fn copy_dir_recursive(source: &Path, dest: &Path) -> std::io::Result<()> {
    let mut pending = vec![(source.to_path_buf(), dest.to_path_buf())];
    while let Some((s, d)) = pending.pop() {
        tokio::fs::create_dir_all(&d).await?;
        let mut entries = tokio::fs::read_dir(&s).await?;
        while let Some(entry) = entries.next_entry().await? {
            let sp = entry.path();
            let dp = d.join(entry.file_name());
            if entry.file_type().await?.is_dir() {
                pending.push((sp, dp));
            } else {
                if let Some(p) = dp.parent() {
                    tokio::fs::create_dir_all(p).await?;
                }
                tokio::fs::copy(sp, dp).await?;
            }
        }
    }
    Ok(())
}
