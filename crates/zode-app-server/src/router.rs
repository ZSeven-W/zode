use crate::capabilities::{
    config_list, config_read, hooks_list, mcp_server_status_list, model_list, plugin_list,
    skills_list, skills_read,
};
use crate::command::CommandRegistry;
use crate::error::error;
use crate::fs::{read_file_base64, write_file_base64};
use crate::initialize::{handle_initialize, ConnectionState};
use crate::threads::ThreadRegistry;
use crate::turns::TurnRegistry;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use zode_app_server_protocol::rpc::{
    ErrorObject, JsonRpcError, JsonRpcRequest, JsonRpcResponse, INVALID_PARAMS, METHOD_NOT_FOUND,
    NOT_INITIALIZED,
};
use zode_app_server_protocol::types::{
    CommandExecParams, EmptyResponse, FsCopyParams, FsCreateDirectoryParams, FsGetMetadataParams,
    FsGetMetadataResponse, FsReadDirectoryEntry, FsReadDirectoryParams, FsReadDirectoryResponse,
    FsReadFileParams, FsReadFileResponse, FsRemoveParams, FsWriteFileParams, InitializeParams,
    SkillReadParams, ThreadListResponse, ThreadNameSetParams, ThreadRefParams, ThreadResponse,
    ThreadStartParams, TurnResponse, TurnStartParams,
};

pub struct Router {
    state: ConnectionState,
    zode_home: String,
    threads: ThreadRegistry,
    turns: TurnRegistry,
    commands: CommandRegistry,
}

impl Router {
    pub fn for_tests(zode_home: &str) -> Self {
        Self {
            state: ConnectionState::default(),
            zode_home: zode_home.to_string(),
            threads: ThreadRegistry::default(),
            turns: TurnRegistry::default(),
            commands: CommandRegistry,
        }
    }

    pub fn new(zode_home: String) -> Self {
        Self {
            state: ConnectionState::default(),
            zode_home,
            threads: ThreadRegistry::default(),
            turns: TurnRegistry::default(),
            commands: CommandRegistry,
        }
    }

    pub fn handle_request(
        &mut self,
        request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, JsonRpcError> {
        if request.method == "initialize" {
            let params: InitializeParams =
                parse_params(request.params).map_err(|error| JsonRpcError {
                    id: request.id.clone(),
                    error,
                })?;
            let result = handle_initialize(&mut self.state, params, self.zode_home.clone())
                .map_err(|error| JsonRpcError {
                    id: request.id.clone(),
                    error,
                })?;
            return Ok(JsonRpcResponse {
                id: request.id,
                result: serde_json::to_value(result).unwrap_or(Value::Null),
            });
        }
        if !self.state.initialized {
            return Err(JsonRpcError {
                id: request.id,
                error: error(NOT_INITIALIZED, "Not initialized"),
            });
        }
        let id = request.id;
        let result = match request.method.as_str() {
            "thread/start" => {
                let params: ThreadStartParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let thread = self
                    .threads
                    .start_metadata_only(params, "(untitled)".to_string())
                    .map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                serde_json::to_value(ThreadResponse { thread }).unwrap_or(Value::Null)
            }
            "thread/list" => serde_json::to_value(ThreadListResponse {
                threads: self.threads.list(),
            })
            .unwrap_or(Value::Null),
            "thread/read" | "thread/resume" => {
                let params: ThreadRefParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let thread =
                    self.threads
                        .read(&params.thread_id)
                        .map_err(|error| JsonRpcError {
                            id: id.clone(),
                            error,
                        })?;
                serde_json::to_value(ThreadResponse { thread }).unwrap_or(Value::Null)
            }
            "thread/name/set" => {
                let params: ThreadNameSetParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                self.threads
                    .set_name(&params.thread_id, &params.name)
                    .map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "thread/delete" => {
                let params: ThreadRefParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                self.threads
                    .delete(&params.thread_id)
                    .map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "turn/start" => {
                let params: TurnStartParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                self.threads
                    .read(&params.thread_id)
                    .map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let (turn, _abort) =
                    self.turns
                        .start(&params.thread_id)
                        .map_err(|message| JsonRpcError {
                            id: id.clone(),
                            error: error(INVALID_PARAMS, message),
                        })?;
                serde_json::to_value(TurnResponse { turn }).unwrap_or(Value::Null)
            }
            "fs/readFile" => {
                let params: FsReadFileParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let data_base64 =
                    read_file_base64(Path::new(&params.path)).map_err(|err| JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/readFile: {err}")),
                    })?;
                serde_json::to_value(FsReadFileResponse { data_base64 }).unwrap_or(Value::Null)
            }
            "fs/writeFile" => {
                let params: FsWriteFileParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                write_file_base64(Path::new(&params.path), &params.data_base64).map_err(|err| {
                    JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/writeFile: {err}")),
                    }
                })?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "fs/createDirectory" => {
                let params: FsCreateDirectoryParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let path = Path::new(&params.path);
                if params.recursive.unwrap_or(true) {
                    std::fs::create_dir_all(path)
                } else {
                    std::fs::create_dir(path)
                }
                .map_err(|err| JsonRpcError {
                    id: id.clone(),
                    error: error(INVALID_PARAMS, format!("fs/createDirectory: {err}")),
                })?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "fs/getMetadata" => {
                let params: FsGetMetadataParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let path = Path::new(&params.path);
                let symlink_meta = std::fs::symlink_metadata(path).map_err(|err| JsonRpcError {
                    id: id.clone(),
                    error: error(INVALID_PARAMS, format!("fs/getMetadata: {err}")),
                })?;
                let meta = std::fs::metadata(path).unwrap_or_else(|_| symlink_meta.clone());
                serde_json::to_value(FsGetMetadataResponse {
                    is_directory: meta.is_dir(),
                    is_file: meta.is_file(),
                    is_symlink: symlink_meta.file_type().is_symlink(),
                    created_at_ms: system_time_ms(symlink_meta.created().ok()),
                    modified_at_ms: system_time_ms(symlink_meta.modified().ok()),
                })
                .unwrap_or(Value::Null)
            }
            "fs/readDirectory" => {
                let params: FsReadDirectoryParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let mut entries = Vec::new();
                for entry in
                    std::fs::read_dir(Path::new(&params.path)).map_err(|err| JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/readDirectory: {err}")),
                    })?
                {
                    let entry = entry.map_err(|err| JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/readDirectory: {err}")),
                    })?;
                    let metadata = entry.metadata().map_err(|err| JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/readDirectory: {err}")),
                    })?;
                    entries.push(FsReadDirectoryEntry {
                        file_name: entry.file_name().to_string_lossy().into_owned(),
                        is_directory: metadata.is_dir(),
                        is_file: metadata.is_file(),
                    });
                }
                entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
                serde_json::to_value(FsReadDirectoryResponse { entries }).unwrap_or(Value::Null)
            }
            "fs/remove" => {
                let params: FsRemoveParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                remove_path(Path::new(&params.path), params.recursive, params.force).map_err(
                    |err| JsonRpcError {
                        id: id.clone(),
                        error: error(INVALID_PARAMS, format!("fs/remove: {err}")),
                    },
                )?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "fs/copy" => {
                let params: FsCopyParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                copy_path(
                    Path::new(&params.source_path),
                    Path::new(&params.destination_path),
                    params.recursive.unwrap_or(false),
                )
                .map_err(|err| JsonRpcError {
                    id: id.clone(),
                    error: error(INVALID_PARAMS, format!("fs/copy: {err}")),
                })?;
                serde_json::to_value(EmptyResponse {}).unwrap_or(Value::Null)
            }
            "command/exec" => {
                let params: CommandExecParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let output = self.commands.exec(params).map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            "model/list" => serde_json::to_value(model_list()).unwrap_or(Value::Null),
            "config/read" => {
                let output = config_read().map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            "config/list" => {
                let output = config_list().map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            "skills/list" => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let output = skills_list(&cwd).map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            "skills/read" => {
                let params: SkillReadParams =
                    parse_params(request.params).map_err(|error| JsonRpcError {
                        id: id.clone(),
                        error,
                    })?;
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let output = skills_read(&cwd, params).map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            "hooks/list" => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                serde_json::to_value(hooks_list(&cwd)).unwrap_or(Value::Null)
            }
            "mcpServerStatus/list" => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                serde_json::to_value(mcp_server_status_list(&cwd)).unwrap_or(Value::Null)
            }
            "plugin/list" => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let output = plugin_list(&cwd).map_err(|error| JsonRpcError {
                    id: id.clone(),
                    error,
                })?;
                serde_json::to_value(output).unwrap_or(Value::Null)
            }
            _ => {
                return Err(JsonRpcError {
                    id,
                    error: error(
                        METHOD_NOT_FOUND,
                        format!("Method not found: {}", request.method),
                    ),
                });
            }
        };
        Ok(JsonRpcResponse { id, result })
    }
}

fn parse_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T, ErrorObject> {
    serde_json::from_value(params.unwrap_or(Value::Object(Default::default())))
        .map_err(|err| error(INVALID_PARAMS, format!("Invalid params: {err}")))
}

fn system_time_ms(value: Option<std::time::SystemTime>) -> i64 {
    value
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn remove_path(path: &Path, recursive: Option<bool>, force: Option<bool>) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && force.unwrap_or(true) => {
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        if recursive.unwrap_or(true) {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_dir(path)
        }
    } else {
        std::fs::remove_file(path)
    }
}

fn copy_path(source: &Path, destination: &Path, recursive: bool) -> std::io::Result<()> {
    let metadata = std::fs::metadata(source)?;
    if metadata.is_dir() {
        if !recursive {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "recursive must be true to copy directories",
            ));
        }
        copy_dir_recursive(source, destination)
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, destination).map(|_| ())
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = PathBuf::from(destination).join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}
