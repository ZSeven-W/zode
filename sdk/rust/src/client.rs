use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use zode_app_server_protocol::rpc::{
    ErrorObject, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId,
};
use zode_app_server_protocol::server_requests::{
    ApprovalDecision, ApprovalRequestParams, ApprovalResponseResult,
};
use zode_app_server_protocol::types::{
    ApprovalPolicy, ClientInfo, InitializeParams, InitializeResponse,
};

use crate::ProtocolMethod;

type PendingSender = oneshot::Sender<Result<Value, ErrorObject>>;
type PendingMap = Arc<Mutex<HashMap<RequestId, PendingSender>>>;
type NotificationHandler = Arc<dyn Fn(String, Value) + Send + Sync>;
type ApprovalHandler = Arc<dyn Fn(ApprovalRequestParams) -> ApprovalDecision + Send + Sync>;

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
        let stdin = Arc::new(Mutex::new(stdin));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let notification_handler = Arc::new(RwLock::new(None));
        let approval_handler = Arc::new(RwLock::new(None));
        let reader = tokio::spawn(read_loop(
            stdout,
            Arc::clone(&stdin),
            Arc::clone(&pending),
            Arc::clone(&notification_handler),
            Arc::clone(&approval_handler),
        ));

        Ok(StdioZodeClient {
            child,
            stdin,
            pending,
            notification_handler,
            approval_handler,
            reader,
            next_id: AtomicI64::new(1),
        })
    }
}

pub struct StdioZodeClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: PendingMap,
    notification_handler: Arc<RwLock<Option<NotificationHandler>>>,
    approval_handler: Arc<RwLock<Option<ApprovalHandler>>>,
    reader: JoinHandle<()>,
    next_id: AtomicI64,
}

impl StdioZodeClient {
    pub fn on_notification<F>(&self, handler: F)
    where
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        *self
            .notification_handler
            .write()
            .expect("handler lock poisoned") = Some(Arc::new(handler));
    }

    pub fn on_approval_request<F>(&self, handler: F)
    where
        F: Fn(ApprovalRequestParams) -> ApprovalDecision + Send + Sync + 'static,
    {
        *self
            .approval_handler
            .write()
            .expect("handler lock poisoned") = Some(Arc::new(handler));
    }

    pub async fn initialize(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
        approval_policy: ApprovalPolicy,
    ) -> Result<InitializeResponse, SdkError> {
        self.request(
            "initialize",
            InitializeParams {
                client_info: ClientInfo {
                    name: name.into(),
                    version: version.into(),
                },
                approval_policy,
            },
        )
        .await
    }

    pub async fn initialize_default(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<InitializeResponse, SdkError> {
        self.initialize(name, version, ApprovalPolicy::default())
            .await
    }

    pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R, SdkError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::Relaxed));
        let message = build_request(id.clone(), method, params)?;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), sender);
        if let Err(error) = write_message(&self.stdin, &message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match receiver.await.map_err(|_| SdkError::Closed)? {
            Ok(result) => Ok(serde_json::from_value(result)?),
            Err(error) => Err(SdkError::Rpc(error)),
        }
    }

    pub async fn request_method<P, R>(
        &self,
        method: ProtocolMethod,
        params: P,
    ) -> Result<R, SdkError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request(method.as_str(), params).await
    }

    pub async fn notify<P>(&self, method: &str, params: Option<P>) -> Result<(), SdkError>
    where
        P: Serialize,
    {
        let message =
            JsonRpcMessage::Notification(zode_app_server_protocol::JsonRpcNotification::new(
                method.to_string(),
                params.map(serde_json::to_value).transpose()?,
            ));
        write_message(&self.stdin, &message).await
    }

    pub async fn notify_method<P>(
        &self,
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
        self.reader.abort();
        Ok(())
    }
}

pub(crate) fn build_request<P: Serialize>(
    id: RequestId,
    method: &str,
    params: P,
) -> Result<JsonRpcMessage, SdkError> {
    Ok(JsonRpcMessage::Request(JsonRpcRequest::new(
        id,
        method,
        Some(serde_json::to_value(params)?),
    )))
}

pub(crate) fn parse_frame(line: &str) -> Result<JsonRpcMessage, SdkError> {
    Ok(serde_json::from_str(line)?)
}

async fn write_message(
    stdin: &Arc<Mutex<ChildStdin>>,
    message: &JsonRpcMessage,
) -> Result<(), SdkError> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(&line).await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_loop(
    stdout: tokio::process::ChildStdout,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: PendingMap,
    notification_handler: Arc<RwLock<Option<NotificationHandler>>>,
    approval_handler: Arc<RwLock<Option<ApprovalHandler>>>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = parse_frame(&line) else {
            continue;
        };
        match message {
            JsonRpcMessage::Response(response) => {
                if let Some(sender) = pending.lock().await.remove(&response.id) {
                    let _ = sender.send(Ok(response.result));
                }
            }
            JsonRpcMessage::Error(error) => {
                if let Some(sender) = pending.lock().await.remove(&error.id) {
                    let _ = sender.send(Err(error.error));
                }
            }
            JsonRpcMessage::Notification(notification) => {
                let handler = notification_handler
                    .read()
                    .expect("handler lock poisoned")
                    .clone();
                if let Some(handler) = handler {
                    let params = notification.params.unwrap_or(Value::Null);
                    tokio::spawn(async move { handler(notification.method, params) });
                }
            }
            JsonRpcMessage::Request(request) if request.method == "approval/request" => {
                let handler = approval_handler
                    .read()
                    .expect("handler lock poisoned")
                    .clone();
                tokio::spawn(handle_approval(stdin.clone(), request, handler));
            }
            JsonRpcMessage::Request(_) => {}
        }
    }
    pending.lock().await.clear();
}

async fn handle_approval(
    stdin: Arc<Mutex<ChildStdin>>,
    request: JsonRpcRequest,
    handler: Option<ApprovalHandler>,
) {
    let params = request
        .params
        .and_then(|value| serde_json::from_value::<ApprovalRequestParams>(value).ok());
    let decision = match (handler, params) {
        (Some(handler), Some(params)) => tokio::spawn(async move { handler(params) })
            .await
            .unwrap_or(ApprovalDecision::Deny),
        _ => ApprovalDecision::Deny,
    };
    let result = serde_json::to_value(ApprovalResponseResult { decision })
        .expect("approval response must serialize");
    let response = JsonRpcMessage::Response(JsonRpcResponse::new(request.id, result));
    let _ = write_message(&stdin, &response).await;
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
