use crate::error::{error, ServerResult};
use std::process::Command;
use uuid::Uuid;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_app_server_protocol::types::{CommandExecParams, CommandExecResponse};

#[derive(Default)]
pub struct CommandRegistry;

impl CommandRegistry {
    pub fn exec(&mut self, params: CommandExecParams) -> ServerResult<CommandExecResponse> {
        if params.command.is_empty() {
            return Err(error(INVALID_PARAMS, "command/exec requires a command"));
        }

        let mut command = Command::new(&params.command[0]);
        command.args(&params.command[1..]);
        if let Some(cwd) = params.cwd {
            command.current_dir(cwd);
        }

        let output = command
            .output()
            .map_err(|err| error(INVALID_PARAMS, format!("command/exec: {err}")))?;
        Ok(CommandExecResponse {
            process_id: Uuid::new_v4().to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        })
    }

    pub fn exec_for_test(&mut self, command: Vec<String>) -> ServerResult<CommandExecResponse> {
        self.exec(CommandExecParams { command, cwd: None })
    }
}
