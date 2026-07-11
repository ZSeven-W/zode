//! Authenticated loopback WebSocket listener and per-connection sessions.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot, Semaphore};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use zode_app_server_protocol::rpc::{JsonRpcError, RequestId, INVALID_REQUEST};
use zode_app_server_protocol::JsonRpcMessage;

use crate::error::error;
use crate::outbound::{next_batch, outbound};
use crate::runtime::ServerRuntimeOptions;
use crate::server_file::{cleanup_if_owner, generate_token, write_atomic, ServerFile};
use crate::session::{SessionActor, SessionMsg};
use crate::stdio_server::route_inbound;

#[derive(Debug, Clone)]
pub struct WsServerConfig {
    pub addr: SocketAddr,
    pub max_connections: usize,
    pub max_frame_bytes: usize,
    pub drain_timeout_ms: u64,
}

impl Default for WsServerConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:0".parse().expect("valid default address"),
            max_connections: 8,
            max_frame_bytes: 1024 * 1024,
            drain_timeout_ms: 5_000,
        }
    }
}

/// Bind, publish discovery credentials, and serve until `shutdown` resolves.
pub async fn run_ws(
    options: ServerRuntimeOptions,
    config: WsServerConfig,
    shutdown: impl Future<Output = ()> + Send,
    ready: oneshot::Sender<(SocketAddr, PathBuf)>,
) -> io::Result<()> {
    let listener = TcpListener::bind(config.addr).await?;
    let addr = listener.local_addr()?;
    let pid = std::process::id();
    let token = generate_token();
    let dir = PathBuf::from(&options.zode_home);
    let path = write_atomic(
        &dir,
        &ServerFile {
            port: addr.port(),
            pid,
            token: token.clone(),
        },
    )?;
    let cleanup = CleanupGuard { dir, pid };
    if ready.send((addr, path)).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "readiness receiver dropped",
        ));
    }

    let semaphore = Arc::new(Semaphore::new(config.max_connections));
    let max_frame_bytes = config.max_frame_bytes;
    let drain_timeout = std::time::Duration::from_millis(config.drain_timeout_ms);
    let (stop_tx, _) = broadcast::channel::<()>(1);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let permit = semaphore.clone().try_acquire_owned().ok();
                let options = options.clone();
                let token = token.clone();
                let stop = stop_tx.subscribe();
                connections.spawn(async move {
                    let _permit = permit;
                    let capacity_available = _permit.is_some();
                    let callback = move |request: &Request, response: Response| {
                        if !capacity_available {
                            return Err(rejection(StatusCode::SERVICE_UNAVAILABLE, "connection limit reached"));
                        }
                        let supplied = request.headers().get("authorization").map(|value| value.as_bytes());
                        let expected = format!("Bearer {token}");
                        if supplied.is_some_and(|value| constant_time_eq(value, expected.as_bytes())) {
                            Ok(response)
                        } else {
                            Err(rejection(StatusCode::UNAUTHORIZED, "unauthorized"))
                        }
                    };
                    let mut ws_config = WebSocketConfig::default();
                    ws_config.max_message_size = Some(max_frame_bytes);
                    ws_config.max_frame_size = Some(max_frame_bytes);
                    if let Ok(socket) = tokio_tungstenite::accept_hdr_async_with_config(
                        stream, callback, Some(ws_config)
                    ).await {
                        let _ = serve_connection(socket, options, stop).await;
                    }
                });
            }
        }
    }

    drop(listener);
    let _ = stop_tx.send(());
    if tokio::time::timeout(drain_timeout, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
    drop(cleanup);
    Ok(())
}

struct CleanupGuard {
    dir: PathBuf,
    pid: u32,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        cleanup_if_owner(&self.dir, self.pid);
    }
}

fn rejection(status: StatusCode, body: &'static str) -> ErrorResponse {
    let mut response = ErrorResponse::new(Some(body.to_string()));
    *response.status_mut() = status;
    response
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let length = left.len().max(right.len());
    for index in 0..length {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

async fn serve_connection(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    options: ServerRuntimeOptions,
    mut stop: broadcast::Receiver<()>,
) -> io::Result<()> {
    let (mut ws_write, mut ws_read) = socket.split();
    let (out_tx, mut out_rx) = outbound();
    let (actor_tx, actor) = SessionActor::spawn(
        Box::new(options.clone()),
        out_tx.clone(),
        options.zode_home.clone(),
        options.sandbox.clone(),
        options.approval_timeout_ms,
        options.dispatch_join_timeout_ms,
    );

    let mut writer_stop = stop.resubscribe();
    let writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = writer_stop.recv() => {
                    ws_write.send(Message::Close(None)).await.map_err(io::Error::other)?;
                    break;
                }
                batch = next_batch(&mut out_rx) => {
                    let Some(batch) = batch else { break; };
                    for message in batch {
                        let text = serde_json::to_string(&message)
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                        ws_write.send(Message::Text(text)).await.map_err(io::Error::other)?;
                    }
                }
            }
        }
        Ok::<_, io::Error>(())
    });

    loop {
        tokio::select! {
            _ = stop.recv() => break,
            frame = ws_read.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    match zode_app_server_transport::stdio::decode_line(&text) {
                        Ok(message) => {
                            if !route_inbound(message, &actor_tx).await {
                                break;
                            }
                        }
                        Err(_) => send_invalid_request(&out_tx).await,
                    }
                }
                Some(Ok(Message::Binary(_))) => send_invalid_request(&out_tx).await,
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    }

    let _ = actor_tx.send(SessionMsg::Shutdown).await;
    drop(actor_tx);
    actor.await.map_err(io::Error::other)?;
    drop(out_tx);
    writer.await.map_err(io::Error::other)??;
    Ok(())
}

async fn send_invalid_request(outbound: &tokio::sync::mpsc::Sender<JsonRpcMessage>) {
    let _ = outbound
        .send(JsonRpcMessage::Error(JsonRpcError::new(
            RequestId::Null,
            error(INVALID_REQUEST, "invalid frame"),
        )))
        .await;
}
