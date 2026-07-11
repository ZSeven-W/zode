use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use zode_app_server_protocol::rpc::{JsonRpcError, RequestId, INVALID_REQUEST};
use zode_app_server_protocol::server_requests::ApprovalResponseResult;
use zode_app_server_protocol::JsonRpcMessage;
use zode_core::sandbox::SandboxConfig;

use crate::error::error;
use crate::outbound::{outbound, writer_task};
use crate::runtime::ServerRuntimeOptions;
use crate::session::{SessionActor, SessionMsg};
use crate::turn_host::HostFactory;

pub async fn serve<R, W>(
    read: R,
    write: W,
    factory: Box<dyn HostFactory>,
    zode_home: String,
    sandbox: Option<SandboxConfig>,
    approval_timeout_ms: u64,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (out_tx, out_rx) = outbound();
    let writer = tokio::spawn(writer_task(out_rx, write));
    let (actor_tx, actor) = SessionActor::spawn(
        factory,
        out_tx.clone(),
        zode_home,
        sandbox,
        approval_timeout_ms,
    );
    let mut lines = BufReader::new(read).lines();

    while let Some(line) = lines.next_line().await? {
        match zode_app_server_transport::stdio::decode_line(&line) {
            Ok(JsonRpcMessage::Request(request)) => {
                let _ = actor_tx.send(SessionMsg::Rpc(request)).await;
            }
            Ok(JsonRpcMessage::Response(response)) => {
                if let RequestId::String(id) = response.id {
                    if id.starts_with("srv-") {
                        let decision =
                            serde_json::from_value::<ApprovalResponseResult>(response.result)
                                .ok()
                                .map(|result| result.decision);
                        let _ = actor_tx
                            .send(SessionMsg::ClientResponse { id, decision })
                            .await;
                    }
                }
            }
            Ok(JsonRpcMessage::Error(response)) => {
                if let RequestId::String(id) = response.id {
                    if id.starts_with("srv-") {
                        let _ = actor_tx
                            .send(SessionMsg::ClientResponse { id, decision: None })
                            .await;
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {
                let _ = out_tx
                    .send(JsonRpcMessage::Error(JsonRpcError::new(
                        RequestId::Null,
                        error(INVALID_REQUEST, "invalid frame"),
                    )))
                    .await;
            }
        }
    }

    let _ = actor_tx.send(SessionMsg::Shutdown).await;
    drop(actor_tx);
    drop(out_tx);
    actor.await.map_err(std::io::Error::other)?;
    writer.await.map_err(std::io::Error::other)??;
    Ok(())
}

pub async fn run_stdio(options: ServerRuntimeOptions) -> std::io::Result<()> {
    let zode_home = options.zode_home.clone();
    let sandbox = options.sandbox.clone();
    let approval_timeout_ms = options.approval_timeout_ms;
    serve(
        tokio::io::stdin(),
        tokio::io::stdout(),
        Box::new(options),
        zode_home,
        sandbox,
        approval_timeout_ms,
    )
    .await
}
