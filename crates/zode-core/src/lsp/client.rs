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
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex, Notify};

use crate::config::LspServerConfig;

/// How long to wait for a response to a request before giving up.
const REQUEST_TIMEOUT_SECS: u64 = 15;

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;
type Diagnostics = Arc<Mutex<HashMap<String, Vec<Value>>>>;

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
    /// URIs already sent via `didOpen` (so we open each file once).
    open: Mutex<HashSet<String>>,
    root: PathBuf,
    /// Keeps the child alive; killed on drop.
    _child: Child,
}

impl LspClient {
    /// Spawn the server, run the initialize/initialized handshake, and return
    /// a ready client. Errors if the process can't start or initialize fails.
    pub async fn start(lang: String, cfg: &LspServerConfig, root: PathBuf) -> Result<Self, String> {
        let mut child = Command::new(&cfg.command)
            .args(&cfg.args)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", cfg.command))?;

        let stdin = Arc::new(Mutex::new(
            child.stdin.take().ok_or("language server has no stdin")?,
        ));
        let stdout = child.stdout.take().ok_or("language server has no stdout")?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics: Diagnostics = Arc::new(Mutex::new(HashMap::new()));
        let diag_notify = Arc::new(Notify::new());

        tokio::spawn(read_loop(
            BufReader::new(stdout),
            stdin.clone(),
            pending.clone(),
            diagnostics.clone(),
            diag_notify.clone(),
        ));

        let client = Self {
            lang,
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            diagnostics,
            diag_notify,
            open: Mutex::new(HashSet::new()),
            root,
            _child: child,
        };
        client.initialize().await?;
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
        self.request("initialize", params).await?;
        self.notify("initialized", json!({})).await;
        Ok(())
    }

    /// Send a request and await its result (or a mapped error).
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = write_message(&self.stdin, &msg).await {
            self.pending.lock().await.remove(&id);
            return Err(format!("lsp write {method}: {e}"));
        }

        let resp = match tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => return Err(format!("lsp {method}: server closed the connection")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(format!("lsp {method}: timed out"));
            }
        };
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

    /// Ensure `path` has been opened on the server; returns its URI.
    pub async fn ensure_open(&self, path: &Path) -> Result<String, String> {
        let uri = path_to_uri(path);
        {
            let mut open = self.open.lock().await;
            if open.contains(&uri) {
                return Ok(uri);
            }
            open.insert(uri.clone());
        }
        let text = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        // The `languageId` is per-file (derived from its extension), not per
        // server: one server can host several languages — typescript-language-
        // server serves js/jsx/ts/tsx, clangd serves c/cpp/objc — and the
        // server applies different rules per id. Falls back to the server key.
        let language_id = language_id_for(path, &self.lang);
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri, "languageId": language_id, "version": 1, "text": text
            }}),
        )
        .await;
        Ok(uri)
    }

    /// Collect diagnostics for `uri`, waiting up to `wait` for the server to
    /// publish its first batch (servers analyze asynchronously after didOpen).
    pub async fn diagnostics_for(&self, uri: &str, wait: Duration) -> Vec<Value> {
        let deadline = Instant::now() + wait;
        loop {
            // Presence of the key means the server has published at least once
            // (even an empty list = "analyzed, no problems").
            if let Some(d) = self.diagnostics.lock().await.get(uri) {
                return d.clone();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Vec::new();
            }
            let tick = remaining.min(Duration::from_millis(150));
            tokio::select! {
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

/// Background reader: frame messages off stdout and route them.
async fn read_loop(
    mut reader: BufReader<ChildStdout>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    diagnostics: Diagnostics,
    diag_notify: Arc<Notify>,
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
        dispatch(&stdin, &pending, &diagnostics, &diag_notify, msg).await;
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
        assert_eq!(language_id_for(Path::new("a.js"), "typescript"), "javascript");
        assert_eq!(language_id_for(Path::new("a.jsx"), "typescript"), "javascriptreact");
        assert_eq!(language_id_for(Path::new("a.ts"), "typescript"), "typescript");
        assert_eq!(language_id_for(Path::new("a.tsx"), "typescript"), "typescriptreact");
        // clangd hosts c/cpp; `.c` is C even when the server key is cpp.
        assert_eq!(language_id_for(Path::new("a.c"), "cpp"), "c");
        assert_eq!(language_id_for(Path::new("a.cpp"), "cpp"), "cpp");
        // Ambiguous `.h` and unknown extensions defer to the server key.
        assert_eq!(language_id_for(Path::new("a.h"), "cpp"), "cpp");
        assert_eq!(language_id_for(Path::new("a.rs"), "rust"), "rust");
        assert_eq!(language_id_for(Path::new("a.weird"), "go"), "go");
    }
}
