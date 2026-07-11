use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use zode_app_server_protocol::rpc::{ErrorObject, JsonRpcMessage, JsonRpcRequest, RequestId};
use zode_app_server_protocol::types::{
    ApprovalPolicy, ClientInfo, InitializeParams, InitializeResponse,
};

use crate::ProtocolMethod;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientOptions {
    pub binary: String,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            binary: "zode".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZodeClient {
    options: ClientOptions,
}

impl ZodeClient {
    pub fn new(options: ClientOptions) -> Self {
        Self { options }
    }

    pub fn binary(&self) -> &str {
        &self.options.binary
    }

    pub async fn spawn_stdio(&self) -> Result<StdioZodeClient, SdkError> {
        let mut child = Command::new(&self.options.binary)
            .arg("server")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().ok_or(SdkError::MissingPipe("stdin"))?;
        let stdout = child.stdout.take().ok_or(SdkError::MissingPipe("stdout"))?;
        Ok(StdioZodeClient {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }
}

pub struct StdioZodeClient {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl StdioZodeClient {
    pub async fn initialize(
        &mut self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<InitializeResponse, SdkError> {
        self.request(
            "initialize",
            InitializeParams {
                client_info: ClientInfo {
                    name: name.into(),
                    version: version.into(),
                },
                approval_policy: ApprovalPolicy::default(),
            },
        )
        .await
    }

    pub async fn request<P, R>(&mut self, method: &str, params: P) -> Result<R, SdkError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = RequestId::Number(self.next_id);
        self.next_id += 1;
        let request = JsonRpcRequest::new(
            id.clone(),
            method.to_string(),
            Some(serde_json::to_value(params)?),
        );
        self.write_message(&JsonRpcMessage::Request(request))
            .await?;

        loop {
            let Some(line) = self.lines.next_line().await? else {
                return Err(SdkError::Closed);
            };
            match serde_json::from_str::<JsonRpcMessage>(&line)? {
                JsonRpcMessage::Response(response) if response.id == id => {
                    return Ok(serde_json::from_value(response.result)?);
                }
                JsonRpcMessage::Error(error) if error.id == id => {
                    return Err(SdkError::Rpc(error.error));
                }
                _ => {}
            }
        }
    }

    pub async fn request_method<P, R>(
        &mut self,
        method: ProtocolMethod,
        params: P,
    ) -> Result<R, SdkError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request(method.as_str(), params).await
    }

    pub async fn notify<P>(&mut self, method: &str, params: Option<P>) -> Result<(), SdkError>
    where
        P: Serialize,
    {
        let message =
            JsonRpcMessage::Notification(zode_app_server_protocol::JsonRpcNotification::new(
                method.to_string(),
                params.map(serde_json::to_value).transpose()?,
            ));
        self.write_message(&message).await
    }

    pub async fn notify_method<P>(
        &mut self,
        method: ProtocolMethod,
        params: Option<P>,
    ) -> Result<(), SdkError>
    where
        P: Serialize,
    {
        self.notify(method.as_str(), params).await
    }

    pub async fn close(mut self) -> Result<(), SdkError> {
        self.child.kill().await?;
        Ok(())
    }

    async fn write_message(&mut self, message: &JsonRpcMessage) -> Result<(), SdkError> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("missing child process {0} pipe")]
    MissingPipe(&'static str),
    #[error("server closed the connection")]
    Closed,
    #[error("rpc error {0:?}")]
    Rpc(ErrorObject),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
