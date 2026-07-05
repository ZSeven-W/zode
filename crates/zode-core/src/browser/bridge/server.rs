use super::{
    BridgeToken, ClientHello, PairError, Pairing, RpcKind, RpcRequest, RpcResponse, ServerHello,
};
use crate::browser::backend::BrowserError;
use futures_util::{SinkExt, StreamExt};
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
    preferred_port: u16,
}

#[derive(Debug, Default)]
struct State {
    pairing: Option<Pairing>,
    active: Option<ActiveConnection>,
    listen_port: Option<u16>,
    waiters: HashMap<u64, oneshot::Sender<Result<serde_json::Value, BrowserError>>>,
}

#[derive(Debug, Clone)]
struct ActiveConnection {
    id: u64,
    tx: mpsc::UnboundedSender<RpcRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingHandle {
    pub code: String,
    pub port: u16,
}

impl BridgeServer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(State::default())),
            listen_lock: tokio::sync::Mutex::new(()),
            next_rpc_id: AtomicU64::new(1),
            next_connection_id: AtomicU64::new(1),
            preferred_port: DEFAULT_BRIDGE_PORT,
        })
    }

    #[cfg(test)]
    fn new_with_preferred_port(preferred_port: u16) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(State::default())),
            listen_lock: tokio::sync::Mutex::new(()),
            next_rpc_id: AtomicU64::new(1),
            next_connection_id: AtomicU64::new(1),
            preferred_port,
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
        if tx.send(req).is_err() {
            self.remove_waiter(id);
            return Err(BrowserError::Dead("bridge connection closed".into()));
        }

        match tokio::time::timeout(Duration::from_secs(30), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(BrowserError::Dead("bridge connection closed".into())),
            Err(_) => {
                self.remove_waiter(id);
                Err(BrowserError::Timeout(format!("bridge rpc {method}")))
            }
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
            let text = serde_json::to_string(&server_hello)
                .map_err(|e| BrowserError::Protocol(format!("bridge hello encode: {e}")))?;
            let _ = ws.send(Message::text(text)).await;
            return Ok(());
        }
        let connection_id = self.next_connection_id.fetch_add(1, Ordering::SeqCst);
        let (mut write, mut read) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.set_active(connection_id, tx)?;

        if let Err(e) = write
            .send(Message::text(
                serde_json::to_string(&server_hello)
                    .map_err(|e| BrowserError::Protocol(format!("bridge hello encode: {e}")))?,
            ))
            .await
        {
            self.clear_active(connection_id);
            return Err(BrowserError::Protocol(format!("bridge hello send: {e}")));
        }

        let writer = tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let Ok(text) = serde_json::to_string(&req) else {
                    continue;
                };
                if write.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
        });

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => self.handle_text_frame(&text),
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        writer.abort();
        self.clear_active(connection_id);
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

    fn handle_text_frame(&self, text: &str) {
        if serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|t| t == "ping"))
            .unwrap_or(false)
        {
            return;
        }

        let Ok(response) = serde_json::from_str::<RpcResponse>(text) else {
            return;
        };
        let result = match response.error {
            Some(err) => Err(BrowserError::Protocol(err)),
            None => Ok(response.result.unwrap_or(serde_json::Value::Null)),
        };
        let waiter = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.waiters.remove(&response.id));
        if let Some(waiter) = waiter {
            let _ = waiter.send(result);
        }
    }

    fn set_active(
        &self,
        id: u64,
        tx: mpsc::UnboundedSender<RpcRequest>,
    ) -> Result<(), BrowserError> {
        let mut state = self.lock_state()?;
        for (_, waiter) in state.waiters.drain() {
            let _ = waiter.send(Err(BrowserError::Dead("bridge connection replaced".into())));
        }
        state.active = Some(ActiveConnection { id, tx });
        Ok(())
    }

    fn clear_active(&self, id: u64) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.active.as_ref().map(|active| active.id) != Some(id) {
            return;
        }
        state.active = None;
        for (_, waiter) in state.waiters.drain() {
            let _ = waiter.send(Err(BrowserError::Dead("bridge connection closed".into())));
        }
    }

    fn remove_waiter(&self, id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.waiters.remove(&id);
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, State>, BrowserError> {
        self.state
            .lock()
            .map_err(|_| BrowserError::Protocol("bridge state lock poisoned".into()))
    }
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
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;

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

    #[tokio::test]
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
