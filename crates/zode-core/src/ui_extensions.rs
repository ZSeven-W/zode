//! Sandboxed, declarative UI extensions supplied by managed plugins.
//!
//! JavaScript receives a JSON snapshot and may only return host-rendered
//! primitives. It has no filesystem/network bridge and never receives TUI
//! objects or terminal handles.

use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rquickjs::CatchResultExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plugin_package::{installed_ui_renderers, PackageUiSlot, PluginPermissions};

pub const MAX_UI_SCRIPT_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 6;
const MAX_SPANS_PER_LINE: usize = 16;
const MAX_TEXT_BYTES: usize = 2048;
const JS_MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const JS_TIME_LIMIT: Duration = Duration::from_millis(25);
/// Renderers run on the draw path; during a streaming turn the context (token
/// counters) changes on almost every frame, so without a floor a slow plugin
/// could re-run its 25 ms budget per frame. Cached output is reused inside
/// this window.
const RENDER_MIN_INTERVAL: Duration = Duration::from_millis(250);
const MAX_HTTP_RESPONSE_BYTES: usize = 256 * 1024;
const MIN_REFRESH_MS: u64 = 10_000;
const MAX_REFRESH_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Renderer {
    plugin: String,
    path: PathBuf,
    source: String,
    slot: PackageUiSlot,
    permissions: PluginPermissions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataSource {
    key: String,
    #[serde(default = "default_refresh_ms")]
    refresh_interval_ms: u64,
    request: DataRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataRequest {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, DataHeader>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DataHeader {
    Literal(String),
    Secret {
        env: String,
        #[serde(default)]
        prefix: String,
    },
}

#[derive(Debug, Clone)]
struct RegisteredDataSource {
    plugin: String,
    allowed_hosts: Vec<String>,
    allowed_env: Vec<String>,
    source: DataSource,
}

fn default_refresh_ms() -> u64 {
    60_000
}

fn default_timeout_ms() -> u64 {
    3_000
}

fn default_method() -> String {
    "GET".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiTone {
    Default,
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSpan {
    pub text: String,
    #[serde(default)]
    pub tone: Option<UiTone>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSidebarLine {
    #[serde(default)]
    pub spans: Vec<UiSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSidebarPanel {
    #[serde(default)]
    pub lines: Vec<UiSidebarLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiStatusLine {
    #[serde(default)]
    pub spans: Vec<UiSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSidebarContribution {
    pub plugin: String,
    pub lines: Vec<UiSidebarLine>,
}

/// Loads installed renderers once and memoizes output while the context is
/// unchanged. Call `reload` after changing the managed plugin registry.
#[derive(Debug, Default)]
pub struct UiExtensionHost {
    renderers: Vec<Renderer>,
    last_sidebar_context: Option<Value>,
    cached_sidebar: Vec<UiSidebarContribution>,
    last_sidebar_eval: Option<Instant>,
    last_status_context: Option<Value>,
    cached_status: Vec<UiStatusLine>,
    last_status_eval: Option<Instant>,
    warned: HashSet<String>,
    data: Arc<RwLock<BTreeMap<String, BTreeMap<String, Value>>>>,
    data_revision: Arc<AtomicU64>,
    rendered_sidebar_revision: u64,
    rendered_status_revision: u64,
    data_tasks: Vec<tokio::task::AbortHandle>,
}

impl Drop for UiExtensionHost {
    fn drop(&mut self) {
        for task in self.data_tasks.drain(..) {
            task.abort();
        }
    }
}

impl UiExtensionHost {
    pub fn load() -> Self {
        let mut host = Self::default();
        host.reload();
        host
    }

    /// Whether any UI renderer is installed. Callers use this to skip building
    /// the (allocation-heavy) render context on every frame.
    pub fn is_empty(&self) -> bool {
        self.renderers.is_empty()
    }

    /// Re-read the managed plugin registry and renderer scripts. Cheap when
    /// nothing changed (a pure re-read and compare); a real change tears down
    /// the background data tasks and caches and rebuilds them, so this is
    /// safe to call on every engine reassembly to pick up plugin
    /// enable/disable/install done since the last load.
    pub fn reload(&mut self) {
        let renderers: Vec<Renderer> = installed_ui_renderers()
            .into_iter()
            .filter_map(|renderer| match std::fs::read_to_string(&renderer.path) {
                Ok(source) if source.len() <= MAX_UI_SCRIPT_BYTES => Some(Renderer {
                    plugin: renderer.plugin,
                    path: renderer.path,
                    source,
                    slot: renderer.slot,
                    permissions: renderer.permissions,
                }),
                Ok(_) => {
                    tracing::warn!(
                        plugin = %renderer.plugin,
                        path = %renderer.path.display(),
                        "skip oversized UI plugin script"
                    );
                    None
                }
                Err(error) => {
                    tracing::warn!(
                        plugin = %renderer.plugin,
                        path = %renderer.path.display(),
                        "skip unreadable UI plugin script: {error}"
                    );
                    None
                }
            })
            .collect();
        if renderers == self.renderers {
            return;
        }
        for task in self.data_tasks.drain(..) {
            task.abort();
        }
        self.renderers = renderers;
        self.last_sidebar_context = None;
        self.cached_sidebar.clear();
        self.last_status_context = None;
        self.cached_status.clear();
        self.warned.clear();
        if let Ok(mut data) = self.data.write() {
            data.clear();
        }
        self.data_revision.fetch_add(1, Ordering::Relaxed);
        self.start_data_sources();
    }

    pub fn sidebar(&mut self, context: Value) -> &[UiSidebarContribution] {
        let revision = self.data_revision();
        if self.last_sidebar_context.as_ref() == Some(&context)
            && self.rendered_sidebar_revision == revision
        {
            return &self.cached_sidebar;
        }
        // Context changed (streaming token counters change it on nearly every
        // frame): floor the JS re-evaluation rate; the stale cache is at most
        // RENDER_MIN_INTERVAL old. `last_sidebar_context` is left untouched so
        // the next call past the window re-evaluates.
        if self
            .last_sidebar_eval
            .is_some_and(|at| at.elapsed() < RENDER_MIN_INTERVAL)
        {
            return &self.cached_sidebar;
        }
        self.last_sidebar_eval = Some(Instant::now());
        self.last_sidebar_context = Some(context.clone());
        self.rendered_sidebar_revision = revision;
        let renderers = self.renderers.clone();
        self.cached_sidebar = renderers
            .iter()
            .filter(|renderer| renderer.slot == PackageUiSlot::Sidebar)
            .filter_map(|renderer| {
                let context = self.context_for_renderer(&context, renderer);
                match run_sidebar_renderer(&renderer.source, &context) {
                    Ok(panel) if panel.lines.is_empty() => None,
                    Ok(panel) => Some(UiSidebarContribution {
                        plugin: renderer.plugin.clone(),
                        lines: panel.lines,
                    }),
                    Err(error) => {
                        let warning_key = format!("{}:sidebar", renderer.plugin);
                        if self.warned.insert(warning_key) {
                            tracing::warn!(
                                plugin = %renderer.plugin,
                                path = %renderer.path.display(),
                                "UI plugin renderer failed: {error}"
                            );
                        }
                        None
                    }
                }
            })
            .collect();
        &self.cached_sidebar
    }

    pub fn status_line(&mut self, context: Value) -> &[UiStatusLine] {
        let revision = self.data_revision();
        if self.last_status_context.as_ref() == Some(&context)
            && self.rendered_status_revision == revision
        {
            return &self.cached_status;
        }
        if self
            .last_status_eval
            .is_some_and(|at| at.elapsed() < RENDER_MIN_INTERVAL)
        {
            return &self.cached_status;
        }
        self.last_status_eval = Some(Instant::now());
        self.last_status_context = Some(context.clone());
        self.rendered_status_revision = revision;
        let renderers = self.renderers.clone();
        self.cached_status = renderers
            .iter()
            .filter(|renderer| renderer.slot == PackageUiSlot::StatusLine)
            .filter_map(|renderer| {
                let context = self.context_for_renderer(&context, renderer);
                match run_status_renderer(&renderer.source, &context) {
                    Ok(line) if line.spans.is_empty() => None,
                    Ok(line) => Some(line),
                    Err(error) => {
                        let warning_key = format!("{}:statusLine", renderer.plugin);
                        if self.warned.insert(warning_key) {
                            tracing::warn!(
                                plugin = %renderer.plugin,
                                path = %renderer.path.display(),
                                "UI plugin status-line renderer failed: {error}"
                            );
                        }
                        None
                    }
                }
            })
            .collect();
        &self.cached_status
    }

    pub fn data_revision(&self) -> u64 {
        self.data_revision.load(Ordering::Relaxed)
    }

    fn context_for_renderer(&self, context: &Value, renderer: &Renderer) -> Value {
        let mut context = context.clone();
        let data = self
            .data
            .read()
            .ok()
            .and_then(|all| all.get(&renderer.plugin).cloned())
            .unwrap_or_default();
        if let Some(object) = context.as_object_mut() {
            for scope in ["tabs", "workspace", "tools", "tasks", "services"] {
                if !renderer
                    .permissions
                    .context
                    .iter()
                    .any(|allowed| allowed == scope)
                {
                    object.remove(scope);
                }
            }
            object.insert(
                "data".into(),
                serde_json::to_value(data).unwrap_or(Value::Null),
            );
        }
        context
    }

    fn start_data_sources(&mut self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            if !self.renderers.is_empty() {
                tracing::debug!(
                    "no tokio runtime on the loading thread — UI plugin data sources stay inactive"
                );
            }
            return;
        };
        let mut seen = HashSet::new();
        let mut seen_keys = HashSet::new();
        let mut sources = Vec::new();
        for renderer in &self.renderers {
            if !seen.insert((renderer.plugin.clone(), renderer.path.clone())) {
                continue;
            }
            match discover_data_sources(&renderer.source) {
                Ok(definitions) => {
                    for source in definitions {
                        match validate_data_source(&renderer.plugin, source, &renderer.permissions)
                        {
                            Ok(source)
                                if seen_keys
                                    .insert((source.plugin.clone(), source.source.key.clone())) =>
                            {
                                sources.push(source)
                            }
                            Ok(source) => tracing::warn!(
                                plugin = %source.plugin,
                                key = %source.source.key,
                                "skip duplicate UI data source key"
                            ),
                            Err(error) => tracing::warn!(
                                plugin = %renderer.plugin,
                                path = %renderer.path.display(),
                                "skip invalid UI data source: {error}"
                            ),
                        }
                    }
                }
                Err(error) => tracing::warn!(
                    plugin = %renderer.plugin,
                    path = %renderer.path.display(),
                    "skip UI plugin data sources: {error}"
                ),
            }
        }
        for source in sources {
            let data = self.data.clone();
            let revision = self.data_revision.clone();
            self.data_tasks.push(
                handle
                    .spawn(async move {
                        loop {
                            let value = fetch_data_source(&source).await;
                            if let Ok(mut all) = data.write() {
                                all.entry(source.plugin.clone())
                                    .or_default()
                                    .insert(source.source.key.clone(), value);
                                revision.fetch_add(1, Ordering::Relaxed);
                            }
                            tokio::time::sleep(Duration::from_millis(
                                source.source.refresh_interval_ms,
                            ))
                            .await;
                        }
                    })
                    .abort_handle(),
            );
        }
    }
}

fn discover_data_sources(source: &str) -> Result<Vec<DataSource>, String> {
    if source.len() > MAX_UI_SCRIPT_BYTES {
        return Err(format!(
            "script exceeds the {MAX_UI_SCRIPT_BYTES}-byte limit"
        ));
    }
    let script = format!(
        r#"
(() => {{
  "use strict";
  const __zodeData = [];
  const register = () => {{}};
  globalThis.zode = Object.freeze({{
    ui: Object.freeze({{ sidebar: register, statusLine: register }}),
    data: Object.freeze({{
      define(key, config) {{
        if (typeof key !== "string" || !key) throw new TypeError("data key must be a string");
        if (!config || typeof config !== "object") throw new TypeError("data config must be an object");
        __zodeData.push({{ key, ...config }});
      }}
    }})
  }});
  {source}
  return JSON.stringify(__zodeData);
}})()
"#
    );
    let raw = eval_js_string(&script)?;
    serde_json::from_str(&raw).map_err(|error| format!("invalid data source: {error}"))
}

fn validate_data_source(
    plugin: &str,
    mut source: DataSource,
    permissions: &PluginPermissions,
) -> Result<RegisteredDataSource, String> {
    if source.key.is_empty()
        || source.key.len() > 64
        || !source
            .key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!("invalid data key {:?}", source.key));
    }
    source.refresh_interval_ms = source
        .refresh_interval_ms
        .clamp(MIN_REFRESH_MS, MAX_REFRESH_MS);
    source.request.timeout_ms = source.request.timeout_ms.clamp(500, 10_000);
    source.request.method = source.request.method.to_ascii_uppercase();
    if !matches!(source.request.method.as_str(), "GET" | "POST") {
        return Err("request method must be GET or POST".into());
    }
    let url = reqwest::Url::parse(&source.request.url)
        .map_err(|error| format!("invalid request URL: {error}"))?;
    if url.scheme() != "https" {
        return Err("request URL must use HTTPS".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("request URL must not contain credentials".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "request URL has no hostname".to_string())?;
    if !permissions
        .network
        .iter()
        .any(|pattern| hostname_matches(pattern, host))
    {
        return Err(format!(
            "hostname {host:?} is not declared in permissions.network"
        ));
    }
    for (name, value) in &source.request.headers {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
        ) {
            return Err(format!("request header {name:?} is controlled by Zode"));
        }
        if let DataHeader::Secret { env, .. } = value {
            if !permissions.env.iter().any(|allowed| allowed == env) {
                return Err(format!(
                    "environment variable {env:?} is not declared in permissions.env"
                ));
            }
        }
    }
    Ok(RegisteredDataSource {
        plugin: plugin.to_string(),
        allowed_hosts: permissions.network.clone(),
        allowed_env: permissions.env.clone(),
        source,
    })
}

fn hostname_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    pattern
        .strip_prefix("*.")
        .is_some_and(|suffix| host != suffix && host.ends_with(&format!(".{suffix}")))
        || host == pattern
}

async fn fetch_data_source(source: &RegisteredDataSource) -> Value {
    match fetch_data_source_inner(source).await {
        Ok((status, data)) => serde_json::json!({
            "ok": (200..300).contains(&status),
            "status": status,
            "data": data,
            "updatedAt": unix_millis(),
        }),
        Err(error) => serde_json::json!({
            "ok": false,
            "error": error,
            "updatedAt": unix_millis(),
        }),
    }
}

async fn fetch_data_source_inner(source: &RegisteredDataSource) -> Result<(u16, Value), String> {
    let url = reqwest::Url::parse(&source.source.request.url)
        .map_err(|error| format!("invalid URL: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "request URL has no hostname".to_string())?;
    if !source
        .allowed_hosts
        .iter()
        .any(|pattern| hostname_matches(pattern, host))
    {
        return Err("request hostname is no longer permitted".into());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("DNS lookup failed: {error}"))?
        .filter(|address| is_public_ip(address.ip()))
        .collect::<Vec<_>>();
    if resolved.is_empty() {
        return Err("hostname did not resolve to a public IP address".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, &resolved)
        .build()
        .map_err(|error| format!("HTTP client: {error}"))?;
    let method = if source.source.request.method == "POST" {
        reqwest::Method::POST
    } else {
        reqwest::Method::GET
    };
    let mut request = client
        .request(method, url)
        .timeout(Duration::from_millis(source.source.request.timeout_ms));
    for (name, value) in &source.source.request.headers {
        let value = match value {
            DataHeader::Literal(value) => value.clone(),
            DataHeader::Secret { env, prefix } => {
                if !source.allowed_env.iter().any(|allowed| allowed == env) {
                    return Err(format!("environment variable {env:?} is not permitted"));
                }
                let secret = std::env::var(env)
                    .map_err(|_| format!("required environment variable {env:?} is not set"))?;
                format!("{prefix}{secret}")
            }
        };
        request = request.header(name, value);
    }
    if let Some(body) = &source.source.request.body {
        request = request.json(body);
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_HTTP_RESPONSE_BYTES as u64)
    {
        return Err(format!("response exceeds {MAX_HTTP_RESPONSE_BYTES} bytes"));
    }
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("response read failed: {error}"))?
    {
        if bytes.len() + chunk.len() > MAX_HTTP_RESPONSE_BYTES {
            return Err(format!("response exceeds {MAX_HTTP_RESPONSE_BYTES} bytes"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let data = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Ok((status, data))
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => {
            if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
                return false;
            }
            if let Some(ip) = ip.to_ipv4_mapped() {
                return is_public_ipv4(ip);
            }
            let segments = ip.segments();
            let embedded_v4 = |high: u16, low: u16| {
                Ipv4Addr::new(
                    (high >> 8) as u8,
                    (high & 0xff) as u8,
                    (low >> 8) as u8,
                    (low & 0xff) as u8,
                )
            };
            // Transition schemes embed an IPv4 address that must pass the same
            // guard: deprecated IPv4-compatible `::a.b.c.d`, the NAT64
            // well-known prefix 64:ff9b::/96, and 6to4 2002::/16.
            if segments[..6] == [0, 0, 0, 0, 0, 0] {
                return is_public_ipv4(embedded_v4(segments[6], segments[7]));
            }
            if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return is_public_ipv4(embedded_v4(segments[6], segments[7]));
            }
            if segments[0] == 0x2002 {
                return is_public_ipv4(embedded_v4(segments[1], segments[2]));
            }
            // Teredo 2001:0::/32 tunnels to an obfuscated IPv4 endpoint that
            // cannot be checked, so it is rejected outright.
            (segments[0] & 0xfe00) != 0xfc00 // unique-local fc00::/7
                && (segments[0] & 0xffc0) != 0xfe80 // link-local fe80::/10
                && (segments[0] & 0xffc0) != 0xfec0 // deprecated site-local fec0::/10
                && !(segments[0] == 0x2001 && segments[1] == 0x0000) // Teredo
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
        }
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run_sidebar_renderer(source: &str, context: &Value) -> Result<UiSidebarPanel, String> {
    let raw = run_renderer(source, context, "sidebar")?;
    let mut panel: UiSidebarPanel =
        serde_json::from_str(&raw).map_err(|error| format!("invalid result: {error}"))?;
    sanitize_sidebar(&mut panel)?;
    Ok(panel)
}

pub fn run_status_renderer(source: &str, context: &Value) -> Result<UiStatusLine, String> {
    let raw = run_renderer(source, context, "statusLine")?;
    let mut line: UiStatusLine =
        serde_json::from_str(&raw).map_err(|error| format!("invalid result: {error}"))?;
    sanitize_spans(&mut line.spans)?;
    Ok(line)
}

fn run_renderer(source: &str, context: &Value, slot: &str) -> Result<String, String> {
    if source.len() > MAX_UI_SCRIPT_BYTES {
        return Err(format!(
            "script exceeds the {MAX_UI_SCRIPT_BYTES}-byte limit"
        ));
    }
    let context_json = serde_json::to_string(context).map_err(|error| error.to_string())?;
    let script = format!(
        r#"
(() => {{
  "use strict";
  const __zodeSlots = Object.create(null);
  const __zodeDeepFreeze = (value) => {{
    if (value && typeof value === "object" && !Object.isFrozen(value)) {{
      Object.freeze(value);
      for (const key of Object.keys(value)) __zodeDeepFreeze(value[key]);
    }}
    return value;
  }};
  globalThis.zode = Object.freeze({{
    ui: Object.freeze({{
      sidebar(renderer) {{
        if (typeof renderer !== "function") throw new TypeError("sidebar expects a function");
        if (__zodeSlots.sidebar) throw new Error("sidebar renderer already registered");
        __zodeSlots.sidebar = renderer;
      }},
      statusLine(renderer) {{
        if (typeof renderer !== "function") throw new TypeError("statusLine expects a function");
        if (__zodeSlots.statusLine) throw new Error("statusLine renderer already registered");
        __zodeSlots.statusLine = renderer;
      }}
    }}),
    data: Object.freeze({{ define() {{}} }})
  }});
  {source}
  const __zodeRenderer = __zodeSlots.{slot} ??
    (typeof render === "function" ? render : null);
  if (!__zodeRenderer) throw new Error("register a renderer with zode.ui.{slot}(fn)");
  const __zodeResult = __zodeRenderer(__zodeDeepFreeze({context_json}));
  return JSON.stringify(
    typeof __zodeResult === "string"
      ? ("{slot}" === "sidebar"
          ? {{ lines: [{{ spans: [{{ text: __zodeResult }}] }}] }}
          : {{ spans: [{{ text: __zodeResult }}] }})
      : __zodeResult
  );
}})()
"#
    );

    eval_js_string(&script)
}

fn eval_js_string(script: &str) -> Result<String, String> {
    let runtime = rquickjs::Runtime::new().map_err(|error| format!("runtime: {error}"))?;
    runtime.set_memory_limit(JS_MEMORY_LIMIT);
    let deadline = Instant::now() + JS_TIME_LIMIT;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let js = rquickjs::Context::full(&runtime).map_err(|error| format!("context: {error}"))?;
    js.with(|ctx| {
        ctx.eval::<String, _>(script.as_bytes())
            .catch(&ctx)
            .map_err(|error| error.to_string())
    })
}

fn sanitize_sidebar(panel: &mut UiSidebarPanel) -> Result<(), String> {
    if panel.lines.len() > MAX_LINES {
        return Err(format!("renderer returned more than {MAX_LINES} lines"));
    }
    for line in &mut panel.lines {
        sanitize_spans(&mut line.spans)?;
    }
    Ok(())
}

fn sanitize_spans(spans: &mut [UiSpan]) -> Result<(), String> {
    if spans.len() > MAX_SPANS_PER_LINE {
        return Err(format!(
            "renderer returned more than {MAX_SPANS_PER_LINE} spans on one line"
        ));
    }
    let mut bytes = 0;
    for span in spans {
        span.text = span
            .text
            .chars()
            .map(|character| {
                if character == '\n' || character == '\r' || character == '\t' {
                    ' '
                } else if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect();
        bytes += span.text.len();
    }
    if bytes > MAX_TEXT_BYTES {
        return Err(format!(
            "renderer text exceeds the {MAX_TEXT_BYTES}-byte limit"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_registered_sidebar() {
        let output = run_sidebar_renderer(
            r#"
zode.ui.sidebar((ctx) => ({
  lines: [
    { spans: [
      { text: ctx.model.id, tone: "accent", bold: true },
      { text: ` ${ctx.context.usedPercent}%`, tone: "muted" }
    ]}
  ]
}));
"#,
            &serde_json::json!({
                "model": {"id": "gpt-test"},
                "context": {"usedPercent": 42}
            }),
        )
        .unwrap();
        assert_eq!(output.lines[0].spans[0].text, "gpt-test");
        assert_eq!(output.lines[0].spans[0].tone, Some(UiTone::Accent));
        assert!(output.lines[0].spans[0].bold);
        assert_eq!(output.lines[0].spans[1].text, " 42%");
    }

    #[test]
    fn supports_string_shorthand_and_sanitizes_controls() {
        let output = run_sidebar_renderer(
            "function render() { return 'hello\\nworld'; }",
            &Value::Null,
        )
        .unwrap();
        assert_eq!(output.lines[0].spans[0].text, "hello world");
    }

    #[test]
    fn rejects_excessive_output() {
        let error = run_sidebar_renderer(
            "zode.ui.sidebar(() => ({lines: [{spans: [{text: 'x'.repeat(3000)}]}]}));",
            &Value::Null,
        )
        .unwrap_err();
        assert!(error.contains("exceeds"));
    }

    #[test]
    fn renders_registered_status_line() {
        let output = run_status_renderer(
            r#"
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent" },
    { text: " synced", tone: "success", italic: true }
  ]
}));
"#,
            &serde_json::json!({"session": {"title": "demo"}}),
        )
        .unwrap();
        assert_eq!(output.spans[0].text, "demo");
        assert_eq!(output.spans[1].tone, Some(UiTone::Success));
        assert!(output.spans[1].italic);
    }

    #[test]
    fn discovers_declarative_data_source_without_running_renderer() {
        let script = r#"
zode.data.define("quota", {
  refreshIntervalMs: 30000,
  request: {
    url: "https://api.example.com/quota",
    headers: {
      Authorization: { env: "PLAN_TOKEN", prefix: "Bearer " }
    }
  }
});
zode.ui.statusLine((ctx) => ({ spans: [{ text: String(ctx.data.quota?.status) }] }));
"#;
        let sources = discover_data_sources(script).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].key, "quota");
        assert_eq!(sources[0].refresh_interval_ms, 30_000);
        assert!(matches!(
            sources[0].request.headers["Authorization"],
            DataHeader::Secret { ref env, .. } if env == "PLAN_TOKEN"
        ));
        let rendered = run_status_renderer(
            script,
            &serde_json::json!({"data": {"quota": {"status": 200}}}),
        )
        .unwrap();
        assert_eq!(rendered.spans[0].text, "200");
    }

    #[test]
    fn data_source_requires_declared_host_and_secret() {
        let source = DataSource {
            key: "quota".into(),
            refresh_interval_ms: 1,
            request: DataRequest {
                url: "https://api.example.com/quota".into(),
                method: "get".into(),
                headers: BTreeMap::from([(
                    "Authorization".into(),
                    DataHeader::Secret {
                        env: "PLAN_TOKEN".into(),
                        prefix: "Bearer ".into(),
                    },
                )]),
                body: None,
                timeout_ms: 1,
            },
        };
        assert!(
            validate_data_source("demo", source.clone(), &PluginPermissions::default())
                .unwrap_err()
                .contains("hostname")
        );
        let registered = validate_data_source(
            "demo",
            source,
            &PluginPermissions {
                network: vec!["*.example.com".into()],
                env: vec!["PLAN_TOKEN".into()],
                context: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(registered.source.refresh_interval_ms, MIN_REFRESH_MS);
        assert_eq!(registered.source.request.timeout_ms, 500);
    }

    #[test]
    fn network_guard_rejects_private_and_documentation_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "203.0.113.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "fec0::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
            "::10.0.0.1",       // IPv4-compatible (deprecated) with private v4
            "64:ff9b::a00:1",   // NAT64 well-known prefix embedding 10.0.0.1
            "2002:c0a8:101::1", // 6to4 embedding 192.168.1.1
            "2001::1",          // Teredo (obfuscated endpoint)
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "2606:4700:4700::1111",
            "64:ff9b::101:101", // NAT64 embedding 1.1.1.1
            "2002:101:101::1",  // 6to4 embedding 1.1.1.1
            "::ffff:8.8.8.8",   // mapped public v4
        ] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn renderer_reevaluation_is_throttled_between_context_changes() {
        let mut host = UiExtensionHost::default();
        host.renderers = vec![Renderer {
            plugin: "demo".into(),
            path: PathBuf::from("ui.js"),
            source: r#"zode.ui.statusLine((ctx) => ({
                spans: [{ text: String(ctx.tokens.output) }]
            }));"#
                .into(),
            slot: PackageUiSlot::StatusLine,
            permissions: PluginPermissions::default(),
        }];
        let first = host
            .status_line(serde_json::json!({"tokens": {"output": 1}}))
            .to_vec();
        assert_eq!(first[0].spans[0].text, "1");
        // A different context inside the throttle window returns the cache…
        let second = host
            .status_line(serde_json::json!({"tokens": {"output": 2}}))
            .to_vec();
        assert_eq!(second[0].spans[0].text, "1");
        // …and re-evaluates once the window has passed (cleared here rather
        // than sleeping RENDER_MIN_INTERVAL in the test).
        host.last_status_eval = None;
        let third = host
            .status_line(serde_json::json!({"tokens": {"output": 3}}))
            .to_vec();
        assert_eq!(third[0].spans[0].text, "3");
    }

    #[test]
    fn richer_context_requires_explicit_scope() {
        let host = UiExtensionHost::default();
        let context = serde_json::json!({
            "session": {"title": "demo"},
            "tools": {"available": ["GitStatus"]},
            "workspace": {"modifiedFiles": []}
        });
        let renderer = |context_scopes| Renderer {
            plugin: "demo".into(),
            path: PathBuf::from("ui.js"),
            source: String::new(),
            slot: PackageUiSlot::StatusLine,
            permissions: PluginPermissions {
                network: Vec::new(),
                env: Vec::new(),
                context: context_scopes,
            },
        };
        let basic = host.context_for_renderer(&context, &renderer(Vec::new()));
        assert!(basic.get("session").is_some());
        assert!(basic.get("tools").is_none());
        assert!(basic.get("workspace").is_none());

        let scoped = host.context_for_renderer(
            &context,
            &renderer(vec!["tools".into(), "workspace".into()]),
        );
        assert_eq!(scoped["tools"]["available"][0], "GitStatus");
        assert!(scoped.get("workspace").is_some());
    }
}
