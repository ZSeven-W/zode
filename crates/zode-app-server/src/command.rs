use crate::error::{error, ServerResult};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use uuid::Uuid;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_app_server_protocol::types::{CommandExecParams, CommandExecResponse};
use zode_core::sandbox::SandboxConfig;

pub const COMMAND_TIMEOUT_MS: u64 = 120_000;
pub const OUTPUT_CAP_BYTES: usize = 1024 * 1024;

pub async fn exec(
    params: CommandExecParams,
    sandbox: Option<&SandboxConfig>,
) -> ServerResult<CommandExecResponse> {
    if params.command.is_empty() {
        return Err(error(INVALID_PARAMS, "command/exec requires a command"));
    }

    let argv = sandbox
        .map(|sandbox| sandbox.wrap_argv(&params.command))
        .unwrap_or(params.command);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    if let Some(cwd) = params.cwd {
        command.current_dir(cwd);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| error(INVALID_PARAMS, format!("command/exec: {err}")))?;
    let stdout = child.stdout.take().expect("piped stdout is available");
    let stderr = child.stderr.take().expect("piped stderr is available");
    let stdout_task = tokio::spawn(read_output(stdout));
    let stderr_task = tokio::spawn(read_output(stderr));
    let timeout_ms = params.timeout_ms.unwrap_or(COMMAND_TIMEOUT_MS);

    let status = match tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait()).await {
        Ok(result) => {
            result.map_err(|err| error(INVALID_PARAMS, format!("command/exec: {err}")))?
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(error(
                INVALID_PARAMS,
                format!("command timed out after {timeout_ms} ms"),
            ));
        }
    };
    let stdout = stdout_task
        .await
        .map_err(|err| error(INVALID_PARAMS, format!("command/exec stdout: {err}")))?
        .map_err(|err| error(INVALID_PARAMS, format!("command/exec stdout: {err}")))?;
    let stderr = stderr_task
        .await
        .map_err(|err| error(INVALID_PARAMS, format!("command/exec stderr: {err}")))?
        .map_err(|err| error(INVALID_PARAMS, format!("command/exec stderr: {err}")))?;

    Ok(CommandExecResponse {
        process_id: Uuid::new_v4().to_string(),
        stdout: truncate_output(stdout),
        stderr: truncate_output(stderr),
        exit_code: status.code(),
    })
}

async fn read_output<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

fn truncate_output(mut bytes: Vec<u8>) -> String {
    let truncated = bytes.len() > OUTPUT_CAP_BYTES;
    if truncated {
        bytes.truncate(OUTPUT_CAP_BYTES);
    }
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        output.push_str("\n[truncated]");
    }
    output
}
