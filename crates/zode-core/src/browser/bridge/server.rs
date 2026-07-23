use super::{
    task_channel::{task_channel, TaskSender},
    task_protocol::TASK_PROTOCOL_VERSION,
    BridgeToken, ClientHello, PairError, Pairing, RpcKind, RpcRequest, RpcResponse, ServerHello,
    TaskClientFrame, TaskInbound, TaskInboundKind, TaskReceiver, TaskServerFrame,
};
use crate::browser::backend::BrowserError;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

pub const DEFAULT_BRIDGE_PORT: u16 = 17657;
pub const EXTENSION_ID: &str = "hcabdgpfhoclfgnknddadgfhhdnlkloc";

#[derive(Debug)]
pub struct BridgeServer {
    state: Arc<Mutex<State>>,
    listen_lock: tokio::sync::Mutex<()>,
    next_rpc_id: AtomicU64,
    next_connection_id: AtomicU64,
    next_invalid_frame_id: AtomicU64,
    preferred_port: u16,
    task_tx: TaskSender,
}

#[derive(Debug, Default)]
struct State {
    pairing: Option<Pairing>,
    active: Option<ActiveConnection>,
    listen_port: Option<u16>,
    waiters: HashMap<u64, oneshot::Sender<Result<serde_json::Value, BrowserError>>>,
    task_receiver: Option<TaskReceiver>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OutboundFrame {
    Browser(RpcRequest),
    Task(TaskServerFrame),
}

#[derive(Debug, Clone)]
struct ActiveConnection {
    id: u64,
    tx: mpsc::UnboundedSender<OutboundFrame>,
}

/// Removes a pending bridge RPC waiter when the caller is cancelled or its
/// future is otherwise dropped. The extension may still have received the
/// request, so the tool boundary separately records unresolved external work;
/// this guard only prevents a stale local waiter from surviving until timeout.
struct RpcWaiterGuard<'a> {
    server: &'a BridgeServer,
    id: u64,
}

impl Drop for RpcWaiterGuard<'_> {
    fn drop(&mut self) {
        self.server.remove_waiter(self.id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingHandle {
    pub code: String,
    pub port: u16,
}

impl BridgeServer {
    pub fn new() -> Arc<Self> {
        let (task_tx, task_receiver) = task_channel();
        Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                task_receiver: Some(task_receiver),
                ..State::default()
            })),
            listen_lock: tokio::sync::Mutex::new(()),
            next_rpc_id: AtomicU64::new(1),
            next_connection_id: AtomicU64::new(1),
            next_invalid_frame_id: AtomicU64::new(1),
            preferred_port: DEFAULT_BRIDGE_PORT,
            task_tx,
        })
    }

    #[cfg(test)]
    fn new_with_preferred_port(preferred_port: u16) -> Arc<Self> {
        let (task_tx, task_receiver) = task_channel();
        Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                task_receiver: Some(task_receiver),
                ..State::default()
            })),
            listen_lock: tokio::sync::Mutex::new(()),
            next_rpc_id: AtomicU64::new(1),
            next_connection_id: AtomicU64::new(1),
            next_invalid_frame_id: AtomicU64::new(1),
            preferred_port,
            task_tx,
        })
    }

    pub async fn ensure_listening(self: &Arc<Self>) -> Result<u16, BrowserError> {
        if let Some(port) = self.lock_state()?.listen_port {
            return Ok(port);
        }
        let _listen_guard = self.listen_lock.lock().await;
        if let Some(port) = self.lock_state()?.listen_port {
            return Ok(port);
        }

        let listener = bind_listener(self.preferred_port).await?;
        let bound_port = listener
            .local_addr()
            .map_err(|e| BrowserError::Launch(format!("bridge local addr: {e}")))?
            .port();
        let (port, should_spawn) = {
            let mut state = self.lock_state()?;
            if let Some(port) = state.listen_port {
                (port, false)
            } else {
                state.listen_port = Some(bound_port);
                (bound_port, true)
            }
        };
        if should_spawn {
            let srv = self.clone();
            tokio::spawn(async move {
                srv.accept_loop(listener).await;
            });
        }

        Ok(port)
    }

    pub async fn start_pairing(self: &Arc<Self>) -> Result<PairingHandle, BrowserError> {
        let pairing = Pairing::new(Instant::now());
        let code = pairing.code().to_string();
        let port = self.ensure_listening().await?;
        self.lock_state()?.pairing = Some(pairing);
        Ok(PairingHandle { code, port })
    }

    pub fn is_connected(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.active.is_some())
            .unwrap_or(false)
    }

    /// Whether `connection_id` is still the authenticated task-channel owner.
    /// Callers use this immediately before applying an asynchronous task result
    /// so a replaced extension connection cannot mutate current state.
    pub fn is_task_connection_active(&self, connection_id: u64) -> bool {
        self.state
            .lock()
            .map(|state| state.active.as_ref().map(|active| active.id) == Some(connection_id))
            .unwrap_or(false)
    }

    pub fn has_saved_token(&self) -> bool {
        BridgeToken::load().is_some()
    }

    pub(crate) fn take_task_receiver(&self) -> Option<TaskReceiver> {
        self.state.lock().ok()?.task_receiver.take()
    }

    pub(crate) fn send_task(
        &self,
        connection_id: u64,
        frame: TaskServerFrame,
    ) -> Result<(), BrowserError> {
        let tx = {
            let state = self.lock_state()?;
            match state.active.as_ref() {
                Some(active) if active.id == connection_id => active.tx.clone(),
                Some(_) => {
                    return Err(BrowserError::Dead(format!(
                        "stale extension task connection {connection_id}"
                    )))
                }
                None => {
                    return Err(BrowserError::Dead(
                        "bridge not connected; run /browser pair".into(),
                    ))
                }
            }
        };
        tx.send(OutboundFrame::Task(frame))
            .map_err(|_| BrowserError::Dead("bridge connection closed".into()))
    }

    pub(crate) async fn call(
        &self,
        kind: RpcKind,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, BrowserError> {
        let id = self.next_rpc_id.fetch_add(1, Ordering::SeqCst);
        let (reply_tx, reply_rx) = oneshot::channel();
        let tx = {
            let mut state = self.lock_state()?;
            let Some(active) = state.active.as_ref() else {
                return Err(BrowserError::Dead(
                    "bridge not connected; run /browser pair".into(),
                ));
            };
            let tx = active.tx.clone();
            state.waiters.insert(id, reply_tx);
            tx
        };
        let req = RpcRequest {
            id,
            kind,
            method: method.to_string(),
            params,
        };
        let _waiter = RpcWaiterGuard { server: self, id };
        if tx.send(OutboundFrame::Browser(req)).is_err() {
            return Err(BrowserError::Dead("bridge connection closed".into()));
        }

        match tokio::time::timeout(Duration::from_secs(30), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(BrowserError::Dead("bridge connection closed".into())),
            Err(_) => Err(BrowserError::Timeout(format!("bridge rpc {method}"))),
        }
    }

    async fn accept_loop(self: Arc<Self>, listener: TcpListener) {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let srv = self.clone();
            tokio::spawn(async move {
                let _ = srv.handle_connection(stream).await;
            });
        }
    }

    #[allow(clippy::result_large_err)]
    async fn handle_connection(self: Arc<Self>, stream: TcpStream) -> Result<(), BrowserError> {
        let expected_origin = format!("chrome-extension://{EXTENSION_ID}");
        let mut ws = accept_hdr_async(stream, move |req: &Request, resp: Response| {
            let origin_ok = req
                .headers()
                .get("origin")
                .and_then(|h| h.to_str().ok())
                .map(|origin| origin == expected_origin)
                .unwrap_or(false);
            if origin_ok {
                Ok(resp)
            } else {
                let mut response = ErrorResponse::new(Some("forbidden origin".into()));
                *response.status_mut() = StatusCode::FORBIDDEN;
                Err(response)
            }
        })
        .await
        .map_err(|e| BrowserError::Protocol(format!("bridge websocket handshake: {e}")))?;

        let hello = read_client_hello(&mut ws).await?;
        let server_hello = self.authenticate(hello)?;
        if matches!(server_hello, ServerHello::Rejected { .. }) {
            let text = encode_server_hello(&server_hello)?;
            let _ = ws.send(Message::text(text)).await;
            return Ok(());
        }
        let connection_id = self.next_connection_id.fetch_add(1, Ordering::SeqCst);
        let (mut write, mut read) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel();
        if let Some(replaced_id) = self.set_active(connection_id, tx)? {
            self.notify_disconnected(replaced_id).await;
        }

        if let Err(e) = write
            .send(Message::text(encode_server_hello(&server_hello)?))
            .await
        {
            if self.clear_active(connection_id) {
                self.notify_disconnected(connection_id).await;
            }
            return Err(BrowserError::Protocol(format!("bridge hello send: {e}")));
        }

        let writer = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                let Ok(text) = serde_json::to_string(&frame) else {
                    continue;
                };
                if write.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
        });

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => self.handle_text_frame(connection_id, &text).await,
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        writer.abort();
        if self.clear_active(connection_id) {
            self.notify_disconnected(connection_id).await;
        }
        Ok(())
    }

    fn authenticate(&self, hello: ClientHello) -> Result<ServerHello, BrowserError> {
        match hello {
            ClientHello::Pair { code } => {
                let token = {
                    let mut state = self.lock_state()?;
                    let Some(pairing) = state.pairing.as_mut() else {
                        return Ok(ServerHello::Rejected {
                            reason: "no active pairing challenge".into(),
                        });
                    };
                    match pairing.redeem(&code, Instant::now()) {
                        Ok(token) => {
                            state.pairing = None;
                            token
                        }
                        Err(err) => {
                            return Ok(ServerHello::Rejected {
                                reason: pair_error_reason(err),
                            });
                        }
                    }
                };
                BridgeToken {
                    token: token.clone(),
                    extension_id: Some(EXTENSION_ID.into()),
                }
                .save()
                .map_err(|e| BrowserError::Protocol(format!("bridge token save: {e}")))?;
                Ok(ServerHello::Paired { token })
            }
            ClientHello::Auth { token } => {
                let ok = BridgeToken::load()
                    .map(|stored| stored.verify(&token))
                    .unwrap_or(false);
                if ok {
                    Ok(ServerHello::Ok)
                } else {
                    Ok(ServerHello::Rejected {
                        reason: "invalid token".into(),
                    })
                }
            }
        }
    }

    async fn handle_text_frame(&self, connection_id: u64, text: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };

        if value.get("channel").and_then(|channel| channel.as_str()) == Some("tasks") {
            let frame = match serde_json::from_value::<TaskClientFrame>(value.clone()) {
                Ok(frame) => frame,
                Err(_) => {
                    let id = value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| self.invalid_frame_id(connection_id));
                    let _ = self.send_task(
                        connection_id,
                        TaskServerFrame::error(id, "invalid_frame", "invalid task request frame"),
                    );
                    return;
                }
            };

            // Bound task ingress at the WebSocket reader. Reserving first makes
            // the socket apply backpressure without allocating an unbounded
            // backlog. A replacement may happen while capacity is unavailable,
            // so validate ownership again only after the permit is acquired.
            let Ok(permit) = self.task_tx.reserve().await else {
                return;
            };
            let Ok(state) = self.state.lock() else {
                return;
            };
            if state.active.as_ref().map(|active| active.id) != Some(connection_id) {
                return;
            }
            permit.send(TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(frame),
            });
            return;
        }

        if value.get("type").and_then(|value| value.as_str()) == Some("ping") {
            return;
        }

        let Ok(response) = serde_json::from_value::<RpcResponse>(value) else {
            return;
        };
        let result = match response.error {
            Some(err) => Err(BrowserError::Protocol(err)),
            None => Ok(response.result.unwrap_or(serde_json::Value::Null)),
        };
        let waiter = self.state.lock().ok().and_then(|mut state| {
            if state.active.as_ref().map(|active| active.id) != Some(connection_id) {
                return None;
            }
            state.waiters.remove(&response.id)
        });
        if let Some(waiter) = waiter {
            let _ = waiter.send(result);
        }
    }

    fn set_active(
        &self,
        id: u64,
        tx: mpsc::UnboundedSender<OutboundFrame>,
    ) -> Result<Option<u64>, BrowserError> {
        let mut state = self.lock_state()?;
        let replaced_id = state.active.as_ref().map(|active| active.id);
        for (_, waiter) in state.waiters.drain() {
            let _ = waiter.send(Err(BrowserError::Dead("bridge connection replaced".into())));
        }
        state.active = Some(ActiveConnection { id, tx });
        Ok(replaced_id)
    }

    fn clear_active(&self, id: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.active.as_ref().map(|active| active.id) != Some(id) {
            return false;
        }
        state.active = None;
        for (_, waiter) in state.waiters.drain() {
            let _ = waiter.send(Err(BrowserError::Dead("bridge connection closed".into())));
        }
        true
    }

    fn remove_waiter(&self, id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.waiters.remove(&id);
        }
    }

    fn invalid_frame_id(&self, connection_id: u64) -> String {
        let sequence = self.next_invalid_frame_id.fetch_add(1, Ordering::Relaxed);
        format!("$server.invalid-frame.{connection_id}.{sequence}")
    }

    async fn notify_disconnected(&self, connection_id: u64) {
        let _ = self
            .task_tx
            .send(TaskInbound {
                connection_id,
                kind: TaskInboundKind::Disconnected,
            })
            .await;
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, State>, BrowserError> {
        self.state
            .lock()
            .map_err(|_| BrowserError::Protocol("bridge state lock poisoned".into()))
    }
}

fn encode_server_hello(hello: &ServerHello) -> Result<String, BrowserError> {
    let mut value = serde_json::to_value(hello)
        .map_err(|error| BrowserError::Protocol(format!("bridge hello encode: {error}")))?;
    if !matches!(hello, ServerHello::Rejected { .. }) {
        let Some(object) = value.as_object_mut() else {
            return Err(BrowserError::Protocol(
                "bridge hello must encode as an object".into(),
            ));
        };
        object.insert(
            "taskProtocol".into(),
            serde_json::Value::from(TASK_PROTOCOL_VERSION),
        );
    }
    serde_json::to_string(&value)
        .map_err(|error| BrowserError::Protocol(format!("bridge hello encode: {error}")))
}

async fn read_client_hello(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
) -> Result<ClientHello, BrowserError> {
    match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text)
            .map_err(|e| BrowserError::Protocol(format!("bridge hello decode: {e}"))),
        Ok(Some(Ok(_))) => Err(BrowserError::Protocol("bridge hello must be text".into())),
        Ok(Some(Err(e))) => Err(BrowserError::Protocol(format!("bridge hello read: {e}"))),
        Ok(None) => Err(BrowserError::Dead("bridge connection closed".into())),
        Err(_) => Err(BrowserError::Timeout("bridge hello".into())),
    }
}

async fn bind_listener(preferred_port: u16) -> Result<TcpListener, BrowserError> {
    let preferred_addr = format!("127.0.0.1:{preferred_port}");
    match TcpListener::bind(&preferred_addr).await {
        Ok(listener) => return Ok(listener),
        Err(err) if preferred_port != 0 && err.kind() == std::io::ErrorKind::AddrInUse => {}
        Err(err) => {
            return Err(BrowserError::Launch(format!(
                "bridge bind {preferred_addr}: {err}"
            )))
        }
    }

    TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| BrowserError::Launch(format!("bridge bind fallback: {e}")))
}

fn pair_error_reason(err: PairError) -> String {
    match err {
        PairError::Expired => "pairing code expired".into(),
        PairError::LockedOut => "pairing locked out".into(),
        PairError::Wrong { remaining } => {
            format!("wrong pairing code; {remaining} attempts remaining")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::bridge::{
        TaskClientFrame, TaskInbound, TaskInboundKind, TaskReceiver, TaskServerBody,
        TaskServerFrame,
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;

    type TestSocket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn connect_with_origin(
        port: u16,
        origin: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let mut req = format!("ws://127.0.0.1:{port}")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert("Origin", origin.parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        ws
    }

    async fn pair_task_client(srv: &Arc<BridgeServer>) -> (TestSocket, String) {
        let handle = srv.start_pairing().await.unwrap();
        let mut ws =
            connect_with_origin(handle.port, &format!("chrome-extension://{}", EXTENSION_ID)).await;
        ws.send(Message::text(
            serde_json::to_string(&ClientHello::Pair { code: handle.code }).unwrap(),
        ))
        .await
        .unwrap();
        let hello: ServerHello =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        let ServerHello::Paired { token } = hello else {
            panic!("expected paired server hello, got {hello:?}");
        };
        (ws, token)
    }

    async fn receive_task_request(receiver: &mut TaskReceiver) -> TaskInbound {
        loop {
            let inbound = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .expect("timed out waiting for task inbound")
                .expect("task channel closed");
            if matches!(inbound.kind, TaskInboundKind::Request(_)) {
                return inbound;
            }
        }
    }

    async fn receive_task_server_frame(ws: &mut TestSocket) -> TaskServerFrame {
        let message = tokio::time::timeout(Duration::from_secs(1), ws.next())
            .await
            .expect("timed out waiting for task server frame")
            .expect("task socket closed")
            .expect("task socket read failed");
        serde_json::from_str(message.to_text().unwrap()).expect("valid task server frame")
    }

    async fn wait_until_task_queue_is_full(srv: &BridgeServer) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while srv.task_tx.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("task queue did not reach capacity");
    }

    async fn fill_task_queue(ws: &mut TestSocket) {
        for id in 0..super::super::task_channel::TASK_CHANNEL_CAPACITY {
            let request =
                TaskClientFrame::request(format!("fill-{id}"), "turn/start", serde_json::json!({}));
            ws.send(Message::text(serde_json::to_string(&request).unwrap()))
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn pair_then_rpc_roundtrip() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let h = srv.start_pairing().await.unwrap();
        let mut ws =
            connect_with_origin(h.port, &format!("chrome-extension://{}", EXTENSION_ID)).await;

        ws.send(Message::text(
            serde_json::to_string(&ClientHello::Pair {
                code: h.code.clone(),
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        let paired: ServerHello =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(paired, ServerHello::Paired { .. }));

        let extn = tokio::spawn(async move {
            let raw = ws.next().await.unwrap().unwrap();
            let rq: RpcRequest = serde_json::from_str(raw.to_text().unwrap()).unwrap();
            let rsp = RpcResponse {
                id: rq.id,
                result: Some(serde_json::json!("https://x.test/")),
                error: None,
            };
            ws.send(Message::text(serde_json::to_string(&rsp).unwrap()))
                .await
                .unwrap();
        });
        let out = srv
            .call(
                RpcKind::Cdp,
                "Page.navigate",
                serde_json::json!({"url":"https://x.test/"}),
            )
            .await
            .unwrap();
        assert_eq!(out, serde_json::json!("https://x.test/"));
        extn.await.unwrap();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn authenticated_hello_advertises_the_task_protocol_version() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let handle = srv.start_pairing().await.unwrap();
        let mut ws =
            connect_with_origin(handle.port, &format!("chrome-extension://{}", EXTENSION_ID)).await;

        ws.send(Message::text(
            serde_json::to_string(&ClientHello::Pair { code: handle.code }).unwrap(),
        ))
        .await
        .unwrap();
        let hello: serde_json::Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();

        assert_eq!(hello["type"], "paired");
        assert_eq!(hello["taskProtocol"], 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn malformed_task_frame_returns_error_then_legacy_rpc_still_roundtrips() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let (mut ws, _) = pair_task_client(&srv).await;

        ws.send(Message::text(
            serde_json::json!({
                "channel": "tasks",
                "kind": "request",
                "id": "malformed-1",
                "method": 7,
                "params": {}
            })
            .to_string(),
        ))
        .await
        .unwrap();
        let error = receive_task_server_frame(&mut ws).await;
        assert_eq!(
            error,
            TaskServerFrame::error("malformed-1", "invalid_frame", "invalid task request frame")
        );

        let call_server = srv.clone();
        let call = tokio::spawn(async move {
            call_server
                .call(RpcKind::Cdp, "Page.getFrameTree", serde_json::json!({}))
                .await
        });
        let raw = ws.next().await.unwrap().unwrap();
        let request: RpcRequest = serde_json::from_str(raw.to_text().unwrap()).unwrap();
        ws.send(Message::text(
            serde_json::to_string(&RpcResponse {
                id: request.id,
                result: Some(serde_json::json!({"frameTree": {}})),
                error: None,
            })
            .unwrap(),
        ))
        .await
        .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), call)
                .await
                .expect("legacy RPC timed out")
                .unwrap()
                .unwrap(),
            serde_json::json!({"frameTree": {}})
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn malformed_task_without_string_id_gets_server_id_and_cannot_consume_rpc_waiter() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let (mut ws, _) = pair_task_client(&srv).await;
        let call_server = srv.clone();
        let mut call = tokio::spawn(async move {
            call_server
                .call(RpcKind::Cdp, "Runtime.evaluate", serde_json::json!({}))
                .await
        });
        let raw = ws.next().await.unwrap().unwrap();
        let request: RpcRequest = serde_json::from_str(raw.to_text().unwrap()).unwrap();

        ws.send(Message::text(
            serde_json::json!({
                "channel": "tasks",
                "kind": "unknown",
                "id": request.id,
                "result": "must not complete the RPC",
                "error": null
            })
            .to_string(),
        ))
        .await
        .unwrap();
        let error = receive_task_server_frame(&mut ws).await;
        let TaskServerBody::Error { id, code, message } = error.body else {
            panic!("expected malformed task error")
        };
        assert!(
            id.starts_with("$server.invalid-frame."),
            "unexpected id: {id}"
        );
        assert_eq!(code, "invalid_frame");
        assert_eq!(message, "invalid task request frame");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut call)
                .await
                .is_err(),
            "malformed task frame consumed an RPC waiter"
        );

        ws.send(Message::text(
            serde_json::to_string(&RpcResponse {
                id: request.id,
                result: Some(serde_json::json!("real RPC result")),
                error: None,
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), call)
                .await
                .expect("real RPC response timed out")
                .unwrap()
                .unwrap(),
            serde_json::json!("real RPC result")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn task_request_reaches_receiver_and_response_returns_on_same_socket() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut ws, _) = pair_task_client(&srv).await;
        let request = TaskClientFrame::request(
            "task-1",
            "turn/start",
            serde_json::json!({"taskId":"side-panel-1"}),
        );

        ws.send(Message::text(serde_json::to_string(&request).unwrap()))
            .await
            .unwrap();
        let inbound = receive_task_request(&mut receiver).await;

        assert_eq!(inbound.kind, TaskInboundKind::Request(request.clone()),);
        let response = TaskServerFrame::response("task-1", serde_json::json!({"accepted":true}));
        srv.send_task(inbound.connection_id, response.clone())
            .unwrap();
        let returned: TaskServerFrame =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(returned, response);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn closing_extension_emits_connection_closed() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut ws, _) = pair_task_client(&srv).await;
        let request = TaskClientFrame::request("task-1", "turn/start", serde_json::json!({}));
        ws.send(Message::text(serde_json::to_string(&request).unwrap()))
            .await
            .unwrap();
        let request_inbound = receive_task_request(&mut receiver).await;

        ws.close(None).await.unwrap();
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("timed out waiting for disconnect")
            .expect("task channel closed");

        assert_eq!(disconnected.connection_id, request_inbound.connection_id);
        assert_eq!(disconnected.kind, TaskInboundKind::Disconnected);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn disconnect_lifecycle_waits_for_a_full_task_queue_instead_of_dropping() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut ws, _) = pair_task_client(&srv).await;
        fill_task_queue(&mut ws).await;
        wait_until_task_queue_is_full(&srv).await;

        ws.close(None).await.unwrap();

        let mut connection_id = None;
        for _ in 0..super::super::task_channel::TASK_CHANNEL_CAPACITY {
            let inbound = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .expect("queued task request timed out")
                .expect("task channel closed");
            connection_id.get_or_insert(inbound.connection_id);
            assert!(matches!(inbound.kind, TaskInboundKind::Request(_)));
        }
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("disconnect must wait for capacity and then arrive")
            .expect("task channel closed");
        assert_eq!(Some(disconnected.connection_id), connection_id);
        assert_eq!(disconnected.kind, TaskInboundKind::Disconnected);
    }

    #[test]
    fn take_task_receiver_succeeds_only_once() {
        let srv = BridgeServer::new_with_preferred_port(0);

        assert!(srv.take_task_receiver().is_some());
        assert!(srv.take_task_receiver().is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stale_connection_cannot_send_to_replacement_socket() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut old_ws, _) = pair_task_client(&srv).await;
        let old_request = TaskClientFrame::request("old-task", "turn/start", serde_json::json!({}));
        old_ws
            .send(Message::text(serde_json::to_string(&old_request).unwrap()))
            .await
            .unwrap();
        let old_inbound = receive_task_request(&mut receiver).await;

        let (mut new_ws, _) = pair_task_client(&srv).await;
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("replacement did not immediately disconnect old connection")
            .expect("task channel closed");
        assert_eq!(disconnected.connection_id, old_inbound.connection_id);
        assert_eq!(disconnected.kind, TaskInboundKind::Disconnected);

        old_ws.close(None).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err(),
            "old connection emitted a duplicate disconnect",
        );

        let new_request = TaskClientFrame::request("new-task", "turn/start", serde_json::json!({}));
        new_ws
            .send(Message::text(serde_json::to_string(&new_request).unwrap()))
            .await
            .unwrap();
        let new_inbound = receive_task_request(&mut receiver).await;
        assert_ne!(old_inbound.connection_id, new_inbound.connection_id);

        let stale_error = srv
            .send_task(
                old_inbound.connection_id,
                TaskServerFrame::response("old-task", serde_json::json!({"stale":true})),
            )
            .unwrap_err();
        assert!(matches!(stale_error, BrowserError::Dead(_)));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), new_ws.next())
                .await
                .is_err(),
            "stale response reached replacement socket",
        );

        let response = TaskServerFrame::response("new-task", serde_json::json!({"accepted":true}));
        srv.send_task(new_inbound.connection_id, response.clone())
            .unwrap();
        let returned: TaskServerFrame =
            serde_json::from_str(new_ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(returned, response);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn replaced_connection_task_request_is_fenced() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut old_ws, _) = pair_task_client(&srv).await;
        let initial = TaskClientFrame::request("old-initial", "turn/start", serde_json::json!({}));
        old_ws
            .send(Message::text(serde_json::to_string(&initial).unwrap()))
            .await
            .unwrap();
        let old_inbound = receive_task_request(&mut receiver).await;

        let (mut new_ws, _) = pair_task_client(&srv).await;
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("replacement did not disconnect old connection")
            .expect("task channel closed");
        assert_eq!(disconnected.connection_id, old_inbound.connection_id);
        assert_eq!(disconnected.kind, TaskInboundKind::Disconnected);

        let stale = TaskClientFrame::request("old-stale", "turn/start", serde_json::json!({}));
        old_ws
            .send(Message::text(serde_json::to_string(&stale).unwrap()))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err(),
            "replaced connection injected a task request",
        );

        let current = TaskClientFrame::request("new-current", "turn/start", serde_json::json!({}));
        new_ws
            .send(Message::text(serde_json::to_string(&current).unwrap()))
            .await
            .unwrap();
        let new_inbound = receive_task_request(&mut receiver).await;
        assert_eq!(new_inbound.kind, TaskInboundKind::Request(current));
        assert_ne!(new_inbound.connection_id, old_inbound.connection_id);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn request_waiting_for_capacity_is_dropped_before_replacement_disconnect() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        let (mut old_ws, _) = pair_task_client(&srv).await;
        fill_task_queue(&mut old_ws).await;
        wait_until_task_queue_is_full(&srv).await;
        let old_connection_id = srv
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .expect("old connection is active")
            .id;

        let blocked = TaskClientFrame::request("blocked-old", "turn/start", serde_json::json!({}));
        old_ws
            .send(Message::text(serde_json::to_string(&blocked).unwrap()))
            .await
            .unwrap();
        tokio::task::yield_now().await;

        let replacement_server = srv.clone();
        let replacement = tokio::spawn(async move { pair_task_client(&replacement_server).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while srv.is_task_connection_active(old_connection_id) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("replacement did not take ownership while lifecycle was backpressured");

        let mut saw_blocked = false;
        for _ in 0..super::super::task_channel::TASK_CHANNEL_CAPACITY {
            let inbound = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .expect("queued task request timed out")
                .expect("task channel closed");
            assert_eq!(inbound.connection_id, old_connection_id);
            if let TaskInboundKind::Request(frame) = inbound.kind {
                saw_blocked |= frame == blocked;
            } else {
                panic!("disconnect arrived before already-enqueued requests");
            }
        }
        assert!(!saw_blocked, "the capacity waiter was not already enqueued");

        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("replacement disconnect timed out")
            .expect("task channel closed");
        assert_eq!(disconnected.connection_id, old_connection_id);
        assert_eq!(disconnected.kind, TaskInboundKind::Disconnected);

        let (mut new_ws, _) = tokio::time::timeout(Duration::from_secs(1), replacement)
            .await
            .expect("replacement handshake waited past lifecycle delivery")
            .expect("replacement task panicked");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err(),
            "stale capacity waiter injected after disconnect"
        );

        let current = TaskClientFrame::request("new-current", "turn/start", serde_json::json!({}));
        new_ws
            .send(Message::text(serde_json::to_string(&current).unwrap()))
            .await
            .unwrap();
        let inbound = receive_task_request(&mut receiver).await;
        assert_eq!(inbound.kind, TaskInboundKind::Request(current));
        assert_ne!(inbound.connection_id, old_connection_id);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn task_connection_active_query_tracks_connect_replace_and_disconnect() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let mut receiver = srv.take_task_receiver().expect("first receiver available");
        assert!(!srv.is_task_connection_active(1));

        let (mut old_ws, _) = pair_task_client(&srv).await;
        let old_request = TaskClientFrame::request("old", "turn/start", serde_json::json!({}));
        old_ws
            .send(Message::text(serde_json::to_string(&old_request).unwrap()))
            .await
            .unwrap();
        let old_inbound = receive_task_request(&mut receiver).await;
        assert!(srv.is_task_connection_active(old_inbound.connection_id));

        let (mut new_ws, _) = pair_task_client(&srv).await;
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("replacement disconnect timed out")
            .expect("task channel closed");
        assert_eq!(disconnected.connection_id, old_inbound.connection_id);
        assert!(!srv.is_task_connection_active(old_inbound.connection_id));

        let new_request = TaskClientFrame::request("new", "turn/start", serde_json::json!({}));
        new_ws
            .send(Message::text(serde_json::to_string(&new_request).unwrap()))
            .await
            .unwrap();
        let new_inbound = receive_task_request(&mut receiver).await;
        assert!(srv.is_task_connection_active(new_inbound.connection_id));

        new_ws.close(None).await.unwrap();
        let disconnected = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("close disconnect timed out")
            .expect("task channel closed");
        assert_eq!(disconnected.connection_id, new_inbound.connection_id);
        assert!(!srv.is_task_connection_active(new_inbound.connection_id));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn replaced_connection_rpc_response_cannot_complete_new_waiter() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let (mut old_ws, _) = pair_task_client(&srv).await;
        let (mut new_ws, _) = pair_task_client(&srv).await;
        let call_server = srv.clone();
        let mut call = tokio::spawn(async move {
            call_server
                .call(RpcKind::Cdp, "Page.getFrameTree", serde_json::json!({}))
                .await
        });
        let raw = new_ws.next().await.unwrap().unwrap();
        let request: RpcRequest = serde_json::from_str(raw.to_text().unwrap()).unwrap();

        old_ws
            .send(Message::text(
                serde_json::to_string(&RpcResponse {
                    id: request.id,
                    result: Some(serde_json::json!("stale")),
                    error: None,
                })
                .unwrap(),
            ))
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut call)
                .await
                .is_err(),
            "replaced connection completed the current RPC waiter",
        );

        new_ws
            .send(Message::text(
                serde_json::to_string(&RpcResponse {
                    id: request.id,
                    result: Some(serde_json::json!("current")),
                    error: None,
                })
                .unwrap(),
            ))
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), call)
            .await
            .expect("current response timed out")
            .unwrap()
            .unwrap();
        assert_eq!(result, serde_json::json!("current"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn dropped_rpc_call_removes_local_waiter() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let (mut ws, _) = pair_task_client(&srv).await;
        let call_server = srv.clone();
        let call = tokio::spawn(async move {
            call_server
                .call(RpcKind::Cdp, "Page.getFrameTree", serde_json::json!({}))
                .await
        });
        let _request = ws.next().await.unwrap().unwrap();
        assert_eq!(srv.state.lock().unwrap().waiters.len(), 1);

        call.abort();
        let _ = call.await;
        assert!(srv.state.lock().unwrap().waiters.is_empty());
    }

    #[tokio::test]
    async fn wrong_origin_is_rejected() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let h = srv.start_pairing().await.unwrap();
        let mut req = format!("ws://127.0.0.1:{}", h.port)
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert("Origin", "https://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
    }

    #[tokio::test]
    async fn call_without_connection_is_dead() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let err = srv
            .call(RpcKind::Cdp, "Page.reload", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, crate::browser::BrowserError::Dead(_)));
    }

    #[tokio::test]
    async fn start_pairing_reuses_listener_port() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let first = srv.start_pairing().await.unwrap();
        let second = srv.start_pairing().await.unwrap();

        assert_eq!(first.port, second.port);
        assert_ne!(first.code, second.code);
    }

    #[tokio::test]
    async fn ensure_listening_is_idempotent_without_pairing() {
        let srv = BridgeServer::new_with_preferred_port(0);
        let first = srv.ensure_listening().await.unwrap();
        let second = srv.ensure_listening().await.unwrap();

        assert_eq!(first, second);

        let mut ws =
            connect_with_origin(first, &format!("chrome-extension://{}", EXTENSION_ID)).await;
        ws.send(Message::text(
            serde_json::to_string(&ClientHello::Pair {
                code: "000000".into(),
            })
            .unwrap(),
        ))
        .await
        .unwrap();
        let rejected: ServerHello =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(rejected, ServerHello::Rejected { .. }));
    }
}
