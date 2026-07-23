//! A single language-server connection: spawns the server process, speaks
//! LSP (JSON-RPC over Content-Length-framed stdio), and exposes a small
//! request/notify surface plus a diagnostics store.
//!
//! One `LspClient` drives one server (e.g. rust-analyzer). A background task
//! reads framed messages off stdout and either fulfils a pending request (by
//! id), answers a server→client request, or files a `publishDiagnostics`
//! notification into the diagnostics map.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex, Notify};

use agent::abort::AbortController;

use crate::config::LspServerConfig;

/// How long to wait for a response to a normal request before giving up.
const REQUEST_TIMEOUT_SECS: u64 = 15;
/// `initialize` gets a longer budget: a cold server (rust-analyzer indexing,
/// bash-language-server loading tree-sitter) can take far longer to hand back
/// its first response than steady-state queries.
const INIT_TIMEOUT_SECS: u64 = 45;
/// How many trailing stderr lines to keep as the death diagnostic.
const STDERR_TAIL_LINES: usize = 5;
/// How long to let the stderr reader finish after stdout hits EOF, so the
/// server's last words make it into the error we report. Generous on purpose:
/// this grace only runs on the server-death error path, and a tight bound
/// races the spawned stderr reader under CI load, occasionally dropping the
/// server's own diagnostic (the only useful one) from the reported error.
const STDERR_DRAIN: Duration = Duration::from_secs(2);

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;
type Diagnostics = Arc<Mutex<HashMap<String, Vec<Value>>>>;
/// Trailing stderr lines, for when the server dies and we must say why.
type StderrTail = Arc<Mutex<Vec<String>>>;

/// Fail-closed receipt for one LSP message that may still be executing in a
/// persistent server. It deliberately does not count the server reader as a
/// turn worker: the server outlives individual turns, so that would make
/// watchdog quiescence impossible. Instead, only an interrupted protocol
/// operation latches unresolved external work.
pub(crate) struct LspOperationGuard {
    abort: AbortController,
    armed: bool,
}

impl LspOperationGuard {
    fn new(abort: &AbortController) -> Self {
        Self {
            abort: abort.clone(),
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for LspOperationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.abort.mark_unresolved_external_work();
        }
    }
}

enum AbortableWriteError {
    Cancelled,
    Io(std::io::Error),
}

#[derive(Debug)]
pub struct LspClient {
    /// LSP `languageId` (e.g. "rust") — also the config key.
    lang: String,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicI64,
    pending: Pending,
    diagnostics: Diagnostics,
    /// Notified whenever new diagnostics land, so a waiter can re-check.
    diag_notify: Arc<Notify>,
    /// Set once the server's stdout closes: the process is gone, so further
    /// requests fail immediately instead of waiting out their timeout.
    dead: Arc<AtomicBool>,
    /// URIs already sent via `didOpen` (so we open each file once).
    open: Mutex<HashSet<String>>,
    root: PathBuf,
    /// Keeps the child alive; killed on drop.
    _child: Child,
    #[cfg(test)]
    write_failure_notify: Option<Arc<Notify>>,
    #[cfg(test)]
    write_success_notify: Option<Arc<Notify>>,
}

impl LspClient {
    /// Spawn the server, run the initialize/initialized handshake, and return
    /// a ready client. Errors if the process can't start or initialize fails.
    pub async fn start(lang: String, cfg: &LspServerConfig, root: PathBuf) -> Result<Self, String> {
        Self::start_inner(lang, cfg, root, None).await
    }

    /// Turn-scoped startup. The persistent client is still session-owned, but
    /// its initialize handshake observes `abort` and records an interrupted
    /// request as unresolved external work.
    pub(crate) async fn start_with_abort(
        lang: String,
        cfg: &LspServerConfig,
        root: PathBuf,
        abort: &AbortController,
    ) -> Result<Self, String> {
        Self::start_inner(lang, cfg, root, Some(abort)).await
    }

    async fn start_inner(
        lang: String,
        cfg: &LspServerConfig,
        root: PathBuf,
        abort: Option<&AbortController>,
    ) -> Result<Self, String> {
        if abort.is_some_and(AbortController::is_aborted) {
            return Err("language server startup cancelled".to_string());
        }
        let mut child = Command::new(&cfg.command)
            .args(&cfg.args)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Captured, not discarded: when a server refuses to start, its
            // stderr is the only thing that says why.
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", cfg.command))?;

        let stdin = Arc::new(Mutex::new(
            child.stdin.take().ok_or("language server has no stdin")?,
        ));
        let stdout = child.stdout.take().ok_or("language server has no stdout")?;
        let stderr = child.stderr.take().ok_or("language server has no stderr")?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics: Diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let diag_notify = Arc::new(Notify::new());
        let dead = Arc::new(AtomicBool::new(false));
        let stderr_tail: StderrTail = Arc::new(Mutex::new(Vec::new()));

        let stderr_task = tokio::spawn(stderr_loop(BufReader::new(stderr), stderr_tail.clone()));
        tokio::spawn(read_loop(
            BufReader::new(stdout),
            stdin.clone(),
            pending.clone(),
            diagnostics.clone(),
            diag_notify.clone(),
            dead.clone(),
            stderr_tail,
            stderr_task,
        ));

        let client = Self {
            lang,
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            diagnostics,
            diag_notify,
            dead,
            open: Mutex::new(HashSet::new()),
            root,
            _child: child,
            #[cfg(test)]
            write_failure_notify: None,
            #[cfg(test)]
            write_success_notify: None,
        };
        if let Some(abort) = abort {
            client.initialize_with_abort(abort).await?;
        } else {
            client.initialize().await?;
        }
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), String> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(&self.root),
            "capabilities": {
                "textDocument": {
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": {}, "references": {}, "documentSymbol": {},
                    "rename": {}, "formatting": {},
                    "publishDiagnostics": { "relatedInformation": true }
                },
                "workspace": { "configuration": true, "workspaceFolders": true }
            },
            "clientInfo": { "name": "zode", "version": env!("CARGO_PKG_VERSION") }
        });
        self.request_with_timeout("initialize", params, INIT_TIMEOUT_SECS)
            .await?;
        self.notify("initialized", json!({})).await;
        Ok(())
    }

    async fn initialize_with_abort(&self, abort: &AbortController) -> Result<(), String> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(&self.root),
            "capabilities": {
                "textDocument": {
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "definition": {}, "references": {}, "documentSymbol": {},
                    "rename": {}, "formatting": {},
                    "publishDiagnostics": { "relatedInformation": true }
                },
                "workspace": { "configuration": true, "workspaceFolders": true }
            },
            "clientInfo": { "name": "zode", "version": env!("CARGO_PKG_VERSION") }
        });
        self.request_with_timeout_and_abort("initialize", params, INIT_TIMEOUT_SECS, abort)
            .await?;
        if abort.is_aborted() {
            return Err("lsp initialize: cancelled".to_string());
        }
        self.notify_with_abort("initialized", json!({}), abort)
            .await?;
        Ok(())
    }

    /// Send a request and await its result (or a mapped error).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT_SECS)
            .await
    }

    /// Send a request owned by one root turn. Cancellation removes the local
    /// waiter and asks the server to cancel the request; because LSP provides
    /// no acknowledgement for that notification, the operation remains
    /// fail-closed as unresolved external work.
    pub(crate) async fn request_with_abort(
        &self,
        method: &str,
        params: Value,
        abort: &AbortController,
    ) -> Result<Value, String> {
        self.request_with_timeout_and_abort(method, params, REQUEST_TIMEOUT_SECS, abort)
            .await
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout_secs: u64,
    ) -> Result<Value, String> {
        self.request_inner(method, params, timeout_secs, None).await
    }

    async fn request_with_timeout_and_abort(
        &self,
        method: &str,
        params: Value,
        timeout_secs: u64,
        abort: &AbortController,
    ) -> Result<Value, String> {
        self.request_inner(method, params, timeout_secs, Some(abort))
            .await
    }

    async fn request_inner(
        &self,
        method: &str,
        params: Value,
        timeout_secs: u64,
        abort: Option<&AbortController>,
    ) -> Result<Value, String> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(format!("lsp {method}: language server is not running"));
        }
        if abort.is_some_and(AbortController::is_aborted) {
            return Err(format!("lsp {method}: cancelled"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let (write_error, mut operation) = if let Some(abort) = abort {
            match write_message_with_abort(&self.stdin, &msg, abort).await {
                Ok(operation) => (None, Some(operation)),
                Err(AbortableWriteError::Cancelled) => {
                    self.pending.lock().await.remove(&id);
                    return Err(format!("lsp {method}: cancelled"));
                }
                Err(AbortableWriteError::Io(error)) => (Some(error), None),
            }
        } else {
            (write_message(&self.stdin, &msg).await.err(), None)
        };
        #[cfg(test)]
        if write_error.is_some() {
            if let Some(notify) = &self.write_failure_notify {
                notify.notify_one();
            }
        } else if let Some(notify) = &self.write_success_notify {
            notify.notify_one();
        }
        if write_error.is_none() {
            if let Some(abort) = abort {
                abort.pulse();
            }
        }

        // A dying server can close stdin before the stdout reader observes
        // EOF and publishes its stderr-backed exit diagnostic. Keep this
        // receiver alive for that drain window and prefer the useful server
        // error; if the reader never reports back, fall through to the
        // original write error below.
        let response_timeout = if write_error.is_some() {
            STDERR_DRAIN + Duration::from_secs(1)
        } else {
            Duration::from_secs(timeout_secs)
        };

        let response = tokio::time::timeout(response_timeout, rx);
        tokio::pin!(response);
        let response = if let Some(abort) = abort {
            tokio::select! {
                biased;
                response = &mut response => response,
                _ = abort.cancelled() => {
                    self.pending.lock().await.remove(&id);
                    // Best effort only: `$/cancelRequest` is a notification,
                    // so even a successful write cannot prove server-side
                    // cancellation. The armed operation guard records that.
                    let _ = tokio::time::timeout(
                        Duration::from_millis(100),
                        self.notify("$/cancelRequest", json!({ "id": id })),
                    )
                    .await;
                    return Err(format!("lsp {method}: cancelled"));
                }
            }
        } else {
            response.await
        };

        let resp = match response {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                if let Some(e) = write_error {
                    return Err(format!("lsp write {method}: {e}"));
                }
                return Err(format!("lsp {method}: server closed the connection"));
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                if let Some(e) = write_error {
                    return Err(format!("lsp write {method}: {e}"));
                }
                return Err(format!("lsp {method}: timed out"));
            }
        };
        if let Some(operation) = &mut operation {
            operation.disarm();
        }
        if let Some(abort) = abort {
            abort.pulse();
        }
        if let Some(err) = resp.get("error") {
            let m = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("error");
            return Err(format!("lsp {method}: {m}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn notify(&self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let _ = write_message(&self.stdin, &msg).await;
    }

    async fn notify_with_abort(
        &self,
        method: &str,
        params: Value,
        abort: &AbortController,
    ) -> Result<(), String> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut operation = write_message_with_abort(&self.stdin, &msg, abort)
            .await
            .map_err(|error| match error {
                AbortableWriteError::Cancelled => format!("lsp {method}: cancelled"),
                AbortableWriteError::Io(error) => format!("lsp write {method}: {error}"),
            })?;
        operation.disarm();
        abort.pulse();
        Ok(())
    }

    /// Ensure `path` has been opened on the server; returns its URI.
    pub async fn ensure_open(&self, path: &Path) -> Result<String, String> {
        let uri = path_to_uri(path);
        if self.open.lock().await.contains(&uri) {
            return Ok(uri);
        }
        let text = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut open = self.open.lock().await;
        if open.contains(&uri) {
            return Ok(uri);
        }
        // The `languageId` is per-file (derived from its extension), not per
        // server: one server can host several languages — typescript-language-
        // server serves js/jsx/ts/tsx, clangd serves c/cpp/objc — and the
        // server applies different rules per id. Falls back to the server key.
        let language_id = language_id_for(path, &self.lang);
        let msg = json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {
                "uri": uri, "languageId": language_id, "version": 1, "text": text
            }
        }});
        write_message(&self.stdin, &msg)
            .await
            .map_err(|e| format!("lsp write textDocument/didOpen: {e}"))?;
        open.insert(uri.clone());
        Ok(uri)
    }

    /// Abort-aware [`Self::ensure_open`]. A newly sent `didOpen` returns an
    /// armed receipt that the enclosing tool must retain until it reaches a
    /// normal terminal result. Dropping the tool between `didOpen` and that
    /// result marks the turn's external state unresolved.
    pub(crate) async fn ensure_open_with_abort(
        &self,
        path: &Path,
        abort: &AbortController,
    ) -> Result<(String, Option<LspOperationGuard>), String> {
        if abort.is_aborted() {
            return Err("lsp textDocument/didOpen: cancelled".to_string());
        }
        let uri = path_to_uri(path);
        let open = tokio::select! {
            biased;
            _ = abort.cancelled() => {
                return Err("lsp textDocument/didOpen: cancelled".to_string());
            }
            open = self.open.lock() => open,
        };
        if open.contains(&uri) {
            return Ok((uri, None));
        }
        drop(open);

        let text = tokio::select! {
            biased;
            _ = abort.cancelled() => {
                return Err("lsp textDocument/didOpen: cancelled".to_string());
            }
            text = tokio::fs::read_to_string(path) => {
                text.map_err(|e| format!("read {}: {e}", path.display()))?
            }
        };
        let mut open = tokio::select! {
            biased;
            _ = abort.cancelled() => {
                return Err("lsp textDocument/didOpen: cancelled".to_string());
            }
            open = self.open.lock() => open,
        };
        if open.contains(&uri) {
            return Ok((uri, None));
        }

        let language_id = language_id_for(path, &self.lang);
        let msg = json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
            "textDocument": {
                "uri": uri, "languageId": language_id, "version": 1, "text": text
            }
        }});
        let operation = write_message_with_abort(&self.stdin, &msg, abort)
            .await
            .map_err(|error| match error {
                AbortableWriteError::Cancelled => "lsp textDocument/didOpen: cancelled".to_string(),
                AbortableWriteError::Io(error) => {
                    format!("lsp write textDocument/didOpen: {error}")
                }
            })?;
        open.insert(uri.clone());
        abort.pulse();
        Ok((uri, Some(operation)))
    }

    /// Collect diagnostics for `uri`, waiting up to `wait` for the server to
    /// publish its first batch (servers analyze asynchronously after didOpen).
    ///
    /// `None` means the server never published within `wait` — it is still
    /// indexing. That is NOT the same as a clean file, and the caller must not
    /// present it as one: a cold rust-analyzer takes far longer to load a
    /// workspace than any wait we're willing to block a tool call for.
    pub async fn diagnostics_for(&self, uri: &str, wait: Duration) -> Option<Vec<Value>> {
        let deadline = Instant::now() + wait;
        loop {
            // Presence of the key means the server has published at least once
            // (even an empty list = "analyzed, no problems").
            if let Some(d) = self.diagnostics.lock().await.get(uri) {
                return Some(d.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let tick = remaining.min(Duration::from_millis(150));
            tokio::select! {
                _ = self.diag_notify.notified() => {}
                _ = tokio::time::sleep(tick) => {}
            }
        }
    }

    /// Abort-aware [`Self::diagnostics_for`] for root-turn tools.
    pub(crate) async fn diagnostics_for_with_abort(
        &self,
        uri: &str,
        wait: Duration,
        abort: &AbortController,
    ) -> Result<Option<Vec<Value>>, String> {
        let deadline = Instant::now() + wait;
        loop {
            let diagnostics = tokio::select! {
                biased;
                _ = abort.cancelled() => {
                    return Err("lsp diagnostics: cancelled".to_string());
                }
                diagnostics = self.diagnostics.lock() => diagnostics,
            };
            if let Some(diagnostics) = diagnostics.get(uri) {
                abort.pulse();
                return Ok(Some(diagnostics.clone()));
            }
            drop(diagnostics);

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let tick = remaining.min(Duration::from_millis(150));
            tokio::select! {
                biased;
                _ = abort.cancelled() => {
                    return Err("lsp diagnostics: cancelled".to_string());
                }
                _ = self.diag_notify.notified() => {}
                _ = tokio::time::sleep(tick) => {}
            }
        }
    }

    /// Resolve a file URI back to a display path (best-effort, strips scheme).
    pub fn uri_to_display(uri: &str) -> String {
        uri.strip_prefix("file://")
            .map(|p| p.to_string())
            .unwrap_or_else(|| uri.to_string())
    }
}

/// Background reader: frame messages off stdout and route them. When the
/// stream ends — the server exited or the pipe broke — fail everything still
/// waiting instead of leaving it parked until its timeout.
#[allow(clippy::too_many_arguments)]
async fn read_loop(
    reader: BufReader<ChildStdout>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    diagnostics: Diagnostics,
    diag_notify: Arc<Notify>,
    dead: Arc<AtomicBool>,
    stderr_tail: StderrTail,
    stderr_task: tokio::task::JoinHandle<()>,
) {
    read_frames(reader, &stdin, &pending, &diagnostics, &diag_notify).await;
    dead.store(true, Ordering::SeqCst);
    // Let the stderr reader catch up so the server's own words become the
    // error, rather than a bare "it exited".
    let _ = tokio::time::timeout(STDERR_DRAIN, stderr_task).await;
    let tail = stderr_tail.lock().await.join("; ");
    let message = if tail.is_empty() {
        "language server exited unexpectedly".to_string()
    } else {
        format!("language server exited unexpectedly: {tail}")
    };
    for (_, tx) in pending.lock().await.drain() {
        let _ = tx.send(json!({ "error": { "message": message.clone() } }));
    }
}

/// Collect the server's stderr, keeping only the last [`STDERR_TAIL_LINES`]
/// lines — a chatty server must not grow this without bound.
async fn stderr_loop(reader: BufReader<ChildStderr>, tail: StderrTail) {
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let mut tail = tail.lock().await;
        if tail.len() == STDERR_TAIL_LINES {
            tail.remove(0);
        }
        tail.push(line);
    }
}

/// Read framed messages until the stream ends.
async fn read_frames(
    mut reader: BufReader<ChildStdout>,
    stdin: &Arc<Mutex<ChildStdin>>,
    pending: &Pending,
    diagnostics: &Diagnostics,
    diag_notify: &Arc<Notify>,
) {
    loop {
        // Read headers until a blank line; only Content-Length matters.
        let mut content_len: usize = 0;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => return, // EOF: server exited
                Ok(_) => {}
                Err(_) => return,
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                content_len = v.trim().parse().unwrap_or(0);
            }
        }
        if content_len == 0 {
            continue;
        }
        let mut body = vec![0u8; content_len];
        if reader.read_exact(&mut body).await.is_err() {
            return;
        }
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        dispatch(stdin, pending, diagnostics, diag_notify, msg).await;
    }
}

async fn dispatch(
    stdin: &Arc<Mutex<ChildStdin>>,
    pending: &Pending,
    diagnostics: &Diagnostics,
    diag_notify: &Arc<Notify>,
    msg: Value,
) {
    let has_method = msg.get("method").is_some();
    if let Some(id) = msg.get("id").and_then(|v| v.as_i64()) {
        if has_method {
            // Server→client request: answer so the server doesn't stall.
            answer_server_request(stdin, id, &msg).await;
        } else if let Some(tx) = pending.lock().await.remove(&id) {
            let _ = tx.send(msg);
        }
        return;
    }
    // Notification.
    if msg.get("method").and_then(|v| v.as_str()) == Some("textDocument/publishDiagnostics") {
        if let Some(params) = msg.get("params") {
            if let Some(uri) = params.get("uri").and_then(|v| v.as_str()) {
                let diags = params
                    .get("diagnostics")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                diagnostics.lock().await.insert(uri.to_string(), diags);
                diag_notify.notify_waiters();
            }
        }
    }
}

/// Reply to server-initiated requests. We don't implement their features, but
/// must respond or the server blocks: `workspace/configuration` wants an array
/// (one entry per item); everything else (registerCapability, progress create)
/// accepts a null result.
async fn answer_server_request(stdin: &Arc<Mutex<ChildStdin>>, id: i64, msg: &Value) {
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let result = if method == "workspace/configuration" {
        let n = msg
            .pointer("/params/items")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(1);
        Value::Array(vec![Value::Null; n])
    } else {
        Value::Null
    };
    let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let _ = write_message(stdin, &resp).await;
}

async fn write_message(stdin: &Arc<Mutex<ChildStdin>>, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut guard = stdin.lock().await;
    guard.write_all(header.as_bytes()).await?;
    guard.write_all(&body).await?;
    guard.flush().await
}

/// Write one message while observing the owning turn. Waiting for the shared
/// stdin lock has not touched the server, so cancellation there is clean. Once
/// the first write may be polled, the returned RAII receipt stays armed until
/// a caller observes a protocol-level terminal result.
async fn write_message_with_abort(
    stdin: &Arc<Mutex<ChildStdin>>,
    msg: &Value,
    abort: &AbortController,
) -> Result<LspOperationGuard, AbortableWriteError> {
    if abort.is_aborted() {
        return Err(AbortableWriteError::Cancelled);
    }
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut stdin = tokio::select! {
        biased;
        _ = abort.cancelled() => {
            return Err(AbortableWriteError::Cancelled);
        }
        stdin = stdin.lock() => stdin,
    };

    let operation = LspOperationGuard::new(abort);
    let write = async {
        stdin.write_all(header.as_bytes()).await?;
        stdin.write_all(&body).await?;
        stdin.flush().await
    };
    tokio::pin!(write);
    let result = tokio::select! {
        biased;
        result = &mut write => result.map_err(AbortableWriteError::Io),
        _ = abort.cancelled() => Err(AbortableWriteError::Cancelled),
    };
    result.map(|()| operation)
}

/// LSP `languageId` for a file, by extension. Disambiguates the languages a
/// single server hosts (js vs ts, c vs cpp); unknown extensions fall back to
/// the server's own key.
fn language_id_for(path: &Path, fallback: &str) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let id = match ext.as_str() {
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "c" => "c",
        "cpp" | "cc" | "cxx" | "c++" => "cpp",
        "hpp" | "hh" | "hxx" => "cpp",
        // `.h` is ambiguous (C or C++ header) — defer to the server's key.
        "h" => fallback,
        "m" => "objective-c",
        "mm" => "objective-cpp",
        "scss" => "scss",
        "less" => "less",
        "yml" => "yaml",
        "" => fallback,
        _ => return fallback.to_string(),
    };
    id.to_string()
}

/// `file://` URI for an absolute path. Spaces are percent-encoded (the common
/// case); other reserved characters are left as-is for simplicity.
pub fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace(' ', "%20");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that dies on startup must fail its pending `initialize` at
    /// once, carrying the process's own stderr as the reason. Before this, the
    /// read loop hit EOF and simply returned, leaving the request's sender
    /// parked in `pending` — so the caller waited out the full 45s
    /// `INIT_TIMEOUT_SECS` only to get a contentless "lsp initialize: timed
    /// out", and did it again on every retry.
    #[cfg(unix)]
    #[tokio::test]
    async fn start_fails_fast_with_stderr_when_the_server_dies() {
        // Absolute path: sibling tests rewrite PATH, and this one must not
        // depend on it.
        let cfg = LspServerConfig {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "echo 'Unknown binary' >&2; exit 1".into()],
            extensions: vec!["rs".into()],
        };
        let began = Instant::now();
        let err = LspClient::start("rust".into(), &cfg, std::env::temp_dir())
            .await
            .expect_err("a server that exits immediately cannot initialize");

        assert!(
            began.elapsed() < Duration::from_secs(5),
            "should fail fast, not wait out the initialize timeout (took {:?})",
            began.elapsed()
        );
        assert!(err.contains("exited"), "{err}");
        assert!(
            err.contains("Unknown binary"),
            "the server's own stderr is the only useful diagnostic: {err}"
        );
    }

    /// If the server dies while a large request is still being written, the
    /// write can observe EPIPE before the stdout reader reports EOF. The
    /// pending request must still be allowed to receive the richer server
    /// death diagnostic instead of returning a bare broken-pipe error.
    #[cfg(unix)]
    #[tokio::test]
    async fn write_failure_prefers_pending_server_exit_diagnostic() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 0.1; exit 1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn short-lived server");
        let stdin = child.stdin.take().expect("child stdin");
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let write_failed = Arc::new(Notify::new());
        let client = LspClient {
            lang: "rust".into(),
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicI64::new(1),
            pending: pending.clone(),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            diag_notify: Arc::new(Notify::new()),
            dead: Arc::new(AtomicBool::new(false)),
            open: Mutex::new(HashSet::new()),
            root: std::env::temp_dir(),
            _child: child,
            write_failure_notify: Some(write_failed.clone()),
            write_success_notify: None,
        };

        let simulated_reader = async {
            write_failed.notified().await;
            let tx = pending
                .lock()
                .await
                .remove(&1)
                .expect("write failure must retain the pending request for the reader");
            let _ = tx.send(json!({
                "error": {
                    "message": "language server exited unexpectedly: Unknown binary"
                }
            }));
        };
        // Larger than a platform pipe buffer, so the write remains pending
        // until the child exits and closes its stdin.
        let request = client.request_with_timeout(
            "initialize",
            json!({ "padding": "x".repeat(4 * 1024 * 1024) }),
            1,
        );
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let (result, ()) = tokio::join!(request, simulated_reader);
            result
        })
        .await
        .expect("write failure and simulated reader must not hang");
        let err = result.expect_err("the short-lived server cannot accept the request");

        assert!(err.contains("exited"), "{err}");
        assert!(err.contains("Unknown binary"), "{err}");
    }

    #[cfg(unix)]
    fn idle_custom_client(write_success_notify: Option<Arc<Notify>>) -> LspClient {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn idle custom language server");
        let stdin = child.stdin.take().expect("child stdin");
        LspClient {
            lang: "custom".into(),
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicI64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            diag_notify: Arc::new(Notify::new()),
            dead: Arc::new(AtomicBool::new(false)),
            open: Mutex::new(HashSet::new()),
            root: std::env::temp_dir(),
            _child: child,
            write_failure_notify: None,
            write_success_notify,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_custom_server_request_after_send_marks_unresolved() {
        let sent = Arc::new(Notify::new());
        let client = idle_custom_client(Some(sent.clone()));
        let abort = AbortController::new();
        let activity = abort.activity();
        let mut request =
            Box::pin(client.request_with_abort("custom/slowRequest", json!({}), &abort));

        tokio::select! {
            _ = sent.notified() => {}
            result = &mut request => panic!("idle server unexpectedly completed request: {result:?}"),
        }
        assert!(!activity.unresolved_external_work());
        assert_eq!(
            activity.active_workers(),
            0,
            "persistent LSP is not turn work"
        );

        drop(request);

        assert!(activity.unresolved_external_work());
        assert_eq!(activity.active_workers(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_custom_server_request_stops_waiter_and_marks_unresolved() {
        let sent = Arc::new(Notify::new());
        let client = idle_custom_client(Some(sent.clone()));
        let abort = AbortController::new();
        let activity = abort.activity();
        let mut request =
            Box::pin(client.request_with_abort("custom/slowRequest", json!({}), &abort));

        tokio::select! {
            _ = sent.notified() => {}
            result = &mut request => panic!("idle server unexpectedly completed request: {result:?}"),
        }
        abort.abort();
        let error = tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .expect("root abort must stop the local request waiter")
            .expect_err("an aborted request cannot succeed");

        assert!(error.contains("cancelled"), "{error}");
        assert!(activity.unresolved_external_work());
        assert!(client.pending.lock().await.is_empty());
        assert_eq!(activity.active_workers(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn normal_custom_server_response_disarms_request_receipt() {
        let sent = Arc::new(Notify::new());
        let client = idle_custom_client(Some(sent.clone()));
        let abort = AbortController::new();
        let pending = client.pending.clone();
        let request = client.request_with_abort("custom/quickRequest", json!({}), &abort);
        let respond = async {
            sent.notified().await;
            let sender = pending
                .lock()
                .await
                .remove(&1)
                .expect("sent request must have a pending response slot");
            sender
                .send(json!({ "jsonrpc": "2.0", "id": 1, "result": { "ok": true } }))
                .expect("request future still receives its response");
        };

        let (result, ()) = tokio::join!(request, respond);

        assert_eq!(result.expect("normal response"), json!({ "ok": true }));
        assert!(!abort.activity().unresolved_external_work());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_after_did_open_marks_custom_server_work_unresolved() {
        let client = idle_custom_client(None);
        let abort = AbortController::new();
        let file = tempfile::NamedTempFile::new().expect("temp source file");

        let (_, operation) = client
            .ensure_open_with_abort(file.path(), &abort)
            .await
            .expect("didOpen writes to the custom server");
        assert!(!abort.activity().unresolved_external_work());

        drop(operation);

        assert!(abort.activity().unresolved_external_work());
    }

    /// A server that has not published yet must be reported as such, not as an
    /// empty (i.e. clean) result. A cold rust-analyzer spends far longer than
    /// `DIAG_WAIT_SECS` loading the workspace before it says anything, so
    /// "nothing published" is the common first answer — and "no errors" would
    /// be a lie.
    #[cfg(unix)]
    #[tokio::test]
    async fn pending_diagnostics_are_distinct_from_a_clean_file() {
        // A server that initializes and then says nothing more.
        let cfg = LspServerConfig {
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                // Answer `initialize` (id 1), then idle.
                r#"read -r a; read -r b; body='{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'
                   printf 'Content-Length: %d\r\n\r\n%s' ${#body} "$body"; sleep 30"#
                    .into(),
            ],
            extensions: vec!["rs".into()],
        };
        let client = LspClient::start("rust".into(), &cfg, std::env::temp_dir())
            .await
            .expect("handshake completes");

        let pending = client
            .diagnostics_for("file:///x.rs", Duration::from_millis(200))
            .await;
        assert!(pending.is_none(), "never published ≠ published nothing");
    }

    #[test]
    fn uri_round_trips_absolute_path() {
        let uri = path_to_uri(Path::new("/Users/x/main.rs"));
        assert_eq!(uri, "file:///Users/x/main.rs");
        assert_eq!(LspClient::uri_to_display(&uri), "/Users/x/main.rs");
    }

    #[test]
    fn uri_encodes_spaces() {
        let uri = path_to_uri(Path::new("/a b/c.rs"));
        assert_eq!(uri, "file:///a%20b/c.rs");
    }

    #[test]
    fn language_id_disambiguates_by_extension() {
        // One server (typescript-language-server) hosts four ids.
        assert_eq!(
            language_id_for(Path::new("a.js"), "typescript"),
            "javascript"
        );
        assert_eq!(
            language_id_for(Path::new("a.jsx"), "typescript"),
            "javascriptreact"
        );
        assert_eq!(
            language_id_for(Path::new("a.ts"), "typescript"),
            "typescript"
        );
        assert_eq!(
            language_id_for(Path::new("a.tsx"), "typescript"),
            "typescriptreact"
        );
        // clangd hosts c/cpp; `.c` is C even when the server key is cpp.
        assert_eq!(language_id_for(Path::new("a.c"), "cpp"), "c");
        assert_eq!(language_id_for(Path::new("a.cpp"), "cpp"), "cpp");
        // Ambiguous `.h` and unknown extensions defer to the server key.
        assert_eq!(language_id_for(Path::new("a.h"), "cpp"), "cpp");
        assert_eq!(language_id_for(Path::new("a.rs"), "rust"), "rust");
        assert_eq!(language_id_for(Path::new("a.weird"), "go"), "go");
    }
}
