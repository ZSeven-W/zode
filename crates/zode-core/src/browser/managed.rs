//! Managed backend: zode-owned Chrome instance over CDP via
//! chromiumoxide, isolated persistent profile (Chrome 136+ forbids CDP
//! on the default profile — this is also the M1 login-state story).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams;
use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
use chromiumoxide::cdp::browser_protocol::network::{
    EventRequestWillBeSent, EventResponseReceived, RequestId,
};
use chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;
use chromiumoxide::cdp::js_protocol::runtime::{EvaluateParams, ReleaseObjectParams};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::layout::Point;
use chromiumoxide::BrowserConfig as CdpBrowserConfig;
use chromiumoxide::Page;
use futures::StreamExt;

use crate::config::BrowserConfig;

use super::backend::*;
use super::executable::locate_managed_executable;
use super::session::BackendFactory;
use super::snapshot_js::SNAPSHOT_JS;

const OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const NAV_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Cap for the console/network ring buffers and the request-correlation
/// pending queue; oldest entries are evicted once exceeded.
const LOG_BUFFER_CAP: usize = 500;

/// Expand a leading `~/` against the home directory; anything else is
/// returned unchanged (mirrors `hooks_config::expand_tilde`, which is
/// private to that module and not reusable here).
fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// Creates the profile dir with owner-only permissions (mode 0700) from the
/// start, rather than the default umask followed by a best-effort chmod —
/// closes the window where the dir briefly has broader permissions.
/// Falls back to a plain `create_dir_all` on non-unix platforms, which have
/// no POSIX permission bits to set.
fn create_profile_dir(profile: &std::path::Path) -> Result<(), BrowserError> {
    #[cfg(unix)]
    {
        use std::fs::DirBuilder;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(profile)
            .map_err(|e| BrowserError::Launch(format!("profile dir: {e}")))?;
        // `DirBuilder::mode` only governs directories it actually creates;
        // if the profile dir pre-existed (e.g. from an older zode build)
        // with broader permissions, tighten it here too. Best-effort: log
        // rather than swallow, but don't fail launch over it.
        if let Err(e) = std::fs::set_permissions(profile, std::fs::Permissions::from_mode(0o700)) {
            tracing::debug!("profile dir chmod 0700 failed: {e}");
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(profile)
            .map_err(|e| BrowserError::Launch(format!("profile dir: {e}")))?;
    }
    Ok(())
}

/// Dedicated profile dir for the managed browser — never the user's
/// default Chrome profile (Chrome 136+ refuses CDP there anyway).
pub(crate) fn resolve_profile_dir(cfg: &BrowserConfig) -> PathBuf {
    if let Some(dir) = &cfg.profile_dir {
        return expand_tilde(dir);
    }
    crate::config::ConfigManager::config_dir()
        .unwrap_or_else(|_| PathBuf::from(".zode"))
        .join("browser-profile")
}

/// zode-owned Chrome instance driven over CDP.
#[derive(Debug)]
pub struct ManagedBackend {
    // `Browser::close`/`kill` need `&mut self`, but `BrowserBackend::close`
    // only gets `&self` — the mutex supplies the interior mutability.
    browser: tokio::sync::Mutex<Browser>,
    page: tokio::sync::Mutex<Page>,
    alive: Arc<AtomicBool>,
    // Populated by the console/network event listeners (`attach_listeners`)
    // and read by the `console_logs`/`network_log` trait methods.
    console_buf: Arc<StdMutex<VecDeque<ConsoleEntry>>>,
    network_buf: Arc<StdMutex<VecDeque<NetworkEntry>>>,
    downloads: Arc<StdMutex<super::managed_downloads::DownloadCache>>,
    // Last accessibility-snapshot ref counter, for click-by-ref validation.
    snapshot_refs: StdMutex<u32>,
    // Join handles for the listener tasks bound to the CURRENT page (see
    // `replace_listeners`). Aborted and replaced on every page swap so
    // exactly one page's console/network listeners are ever live — without
    // this, a backgrounded tab's listeners keep running and cross-
    // contaminate `console_buf`/`network_buf` with the wrong tab's output.
    listener_handles: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

/// Builds [`ManagedBackend`] instances for [`BrowserSession`](super::session::BrowserSession).
#[derive(Debug)]
pub struct ManagedFactory;

#[async_trait]
impl BackendFactory for ManagedFactory {
    async fn create(&self, cfg: &BrowserConfig) -> Result<Arc<dyn BrowserBackend>, BrowserError> {
        ManagedBackend::launch(cfg)
            .await
            .map(|b| b as Arc<dyn BrowserBackend>)
    }
}

impl ManagedBackend {
    pub async fn launch(cfg: &BrowserConfig) -> Result<Arc<Self>, BrowserError> {
        let exe = locate_managed_executable(cfg)?;
        let profile = resolve_profile_dir(cfg);
        create_profile_dir(&profile)?;
        let downloads_dir = crate::config::ConfigManager::config_dir()
            .unwrap_or_else(|_| PathBuf::from(".zode"))
            .join("downloads");
        create_profile_dir(&downloads_dir)?;
        let (w, h) = cfg.viewport();
        let mut builder = CdpBrowserConfig::builder()
            .chrome_executable(&exe)
            .user_data_dir(&profile)
            .window_size(w, h)
            // `window_size` only sets the OS-level launch flag; the CDP
            // device-metrics viewport (what screenshots are captured
            // against) defaults to 800x600 unless set explicitly here too.
            .viewport(Viewport {
                width: w,
                height: h,
                ..Default::default()
            });
        if !cfg.headless() {
            builder = builder.with_head();
        }
        let cdp_cfg = builder
            .build()
            .map_err(|e| BrowserError::Launch(e.to_string()))?;
        let (browser, mut handler) = Browser::launch(cdp_cfg)
            .await
            .map_err(|e| BrowserError::Launch(e.to_string()))?;

        let downloads = super::managed_downloads::configure(&browser, &downloads_dir).await?;

        // Supervisor: the handler stream MUST be polled continuously;
        // stream end == browser gone (crash or user closed the window).
        let alive = Arc::new(AtomicBool::new(true));
        let alive2 = alive.clone();
        tokio::spawn(async move {
            while handler.next().await.is_some() {}
            alive2.store(false, Ordering::SeqCst);
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| BrowserError::Launch(e.to_string()))?;
        // Chrome opens its own initial blank tab on startup in addition to
        // the one just created above; close any such extras so the session
        // starts with exactly one tab (the `tabs()`/`tab_close` "last tab"
        // semantics assume a single-tab baseline).
        if let Ok(pages) = browser.pages().await {
            for p in pages {
                if p.target_id() != page.target_id() {
                    let _ = p.close().await;
                }
            }
        }
        let backend = Arc::new(Self {
            browser: tokio::sync::Mutex::new(browser),
            page: tokio::sync::Mutex::new(page),
            alive,
            console_buf: Arc::new(StdMutex::new(VecDeque::new())),
            network_buf: Arc::new(StdMutex::new(VecDeque::new())),
            downloads,
            snapshot_refs: StdMutex::new(0),
            listener_handles: StdMutex::new(Vec::new()),
        });
        {
            let page = backend.page.lock().await;
            backend.replace_listeners(&page);
        }
        Ok(backend)
    }

    /// (Re)attaches console/network listeners to `page`, first aborting
    /// whatever listener tasks were bound to the previous page. Must be
    /// called every time `self.page` is swapped (`tab_new`/`tab_select`/
    /// `tab_close`'s fallback) — chromiumoxide scopes event listeners to a
    /// single `Page`, so without a fresh attach the new tab's activity
    /// wouldn't be captured; without the abort, the old page's listener
    /// tasks would keep running forever and cross-contaminate the shared
    /// `console_buf`/`network_buf` with a backgrounded tab's output.
    fn replace_listeners(&self, page: &Page) {
        let handles = attach_listeners(page, self.console_buf.clone(), self.network_buf.clone());
        // Lock is held only long enough to swap the Vec — no `.await` inside
        // the critical section, so this is safe under the std mutex.
        let old = std::mem::replace(&mut *self.listener_handles.lock().unwrap(), handles);
        for h in old {
            h.abort();
        }
    }
}

/// Push `item` onto the back of a ring buffer, evicting the oldest entry
/// once `LOG_BUFFER_CAP` is reached.
fn push_capped<T>(buf: &StdMutex<VecDeque<T>>, item: T) {
    let mut guard = buf.lock().unwrap();
    if guard.len() >= LOG_BUFFER_CAP {
        guard.pop_front();
    }
    guard.push_back(item);
}

/// Builds a [`ConsoleEntry`] from a `Runtime.consoleAPICalled` event: level
/// is the call type (`log`/`error`/...), text joins each argument's JSON
/// `value` (falling back to its `description`) with spaces.
fn console_entry_from_event(ev: Arc<EventConsoleApiCalled>) -> ConsoleEntry {
    let level = ev.r#type.as_ref().to_string();
    let text = ev
        .args
        .iter()
        .map(|arg| match &arg.value {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => arg.description.clone().unwrap_or_default(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    ConsoleEntry { level, text }
}

/// Builds a [`NetworkEntry`] from a `Network.responseReceived` event,
/// recovering the HTTP method from the matching `Network.requestWillBeSent`
/// entry in `pending` (removed once consumed). If no match is found (e.g.
/// listener attached mid-flight), `method` is left empty.
fn network_entry_from_event(
    pending: &StdMutex<VecDeque<(RequestId, String, String)>>,
    ev: Arc<EventResponseReceived>,
) -> NetworkEntry {
    let method = {
        let mut guard = pending.lock().unwrap();
        guard
            .iter()
            .position(|(id, _, _)| *id == ev.request_id)
            .map(|idx| guard.remove(idx).expect("index in bounds").1)
            .unwrap_or_default()
    };
    NetworkEntry {
        method,
        url: ev.response.url.clone(),
        status: u16::try_from(ev.response.status).ok(),
        mime: Some(ev.response.mime_type.clone()),
    }
}

/// Spawns console + network log listener tasks for `page` and returns their
/// join handles so the caller ([`ManagedBackend::replace_listeners`]) can
/// abort them once `page` is no longer current. chromiumoxide scopes event
/// listeners to a single `Page`, so this must be re-run every time
/// `self.page` is swapped (see `tab_new`/`tab_select`) or the new tab's
/// activity won't be captured — and the PREVIOUS page's tasks must be
/// aborted or they keep running (and writing into the shared buffers)
/// indefinitely, since the old `Page` handle isn't dropped, only replaced.
fn attach_listeners(
    page: &Page,
    console_buf: Arc<StdMutex<VecDeque<ConsoleEntry>>>,
    network_buf: Arc<StdMutex<VecDeque<NetworkEntry>>>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(3);

    let console_page = page.clone();
    handles.push(tokio::spawn(async move {
        if let Ok(mut events) = console_page.event_listener::<EventConsoleApiCalled>().await {
            while let Some(ev) = events.next().await {
                push_capped(&console_buf, console_entry_from_event(ev));
            }
        }
    }));

    // Network entries need both the request (method) and response
    // (status/mime) events; correlate them through a small pending queue
    // shared between the two listener tasks.
    let pending: Arc<StdMutex<VecDeque<(RequestId, String, String)>>> =
        Arc::new(StdMutex::new(VecDeque::new()));

    let request_page = page.clone();
    let request_pending = pending.clone();
    handles.push(tokio::spawn(async move {
        if let Ok(mut events) = request_page
            .event_listener::<EventRequestWillBeSent>()
            .await
        {
            while let Some(ev) = events.next().await {
                push_capped(
                    &request_pending,
                    (
                        ev.request_id.clone(),
                        ev.request.method.clone(),
                        ev.request.url.clone(),
                    ),
                );
            }
        }
    }));

    let response_page = page.clone();
    handles.push(tokio::spawn(async move {
        if let Ok(mut events) = response_page
            .event_listener::<EventResponseReceived>()
            .await
        {
            while let Some(ev) = events.next().await {
                push_capped(&network_buf, network_entry_from_event(&pending, ev));
            }
        }
    }));

    handles
}

/// Presses a single key via a raw `Input.dispatchKeyEvent` keydown+keyup
/// pair. `Page` in chromiumoxide 0.9.1 does not expose `type_str`/`press_key`
/// publicly (only `Element` does, backed by the crate-private `PageInner`);
/// this reimplements the same two-command sequence on the public
/// `Page::execute` API so it also covers coordinate-only targets that have
/// no `Element` to call through.
async fn dispatch_key(page: &Page, key: &str) -> Result<(), BrowserError> {
    let def = chromiumoxide::keys::get_key_definition(key)
        .ok_or_else(|| BrowserError::Protocol(format!("unknown key: {key}")))?;
    let mut cmd = DispatchKeyEventParams::builder();
    let key_down_type = if let Some(txt) = def.text {
        cmd = cmd.text(txt);
        DispatchKeyEventType::KeyDown
    } else if def.key.len() == 1 {
        cmd = cmd.text(def.key);
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };
    let cmd = cmd
        .key(def.key)
        .code(def.code)
        .windows_virtual_key_code(def.key_code)
        .native_virtual_key_code(def.key_code);
    page.execute(cmd.clone().r#type(key_down_type).build().unwrap())
        .await
        .map_err(|e| BrowserError::Protocol(e.to_string()))?;
    page.execute(cmd.r#type(DispatchKeyEventType::KeyUp).build().unwrap())
        .await
        .map_err(|e| BrowserError::Protocol(e.to_string()))?;
    Ok(())
}

/// Types `text` by pressing one key per character (mirrors chromiumoxide's
/// own `type_str`, reimplemented on top of [`dispatch_key`]).
async fn dispatch_type_str(page: &Page, text: &str) -> Result<(), BrowserError> {
    for c in text.split("").filter(|s| !s.is_empty()) {
        dispatch_key(page, c).await?;
    }
    Ok(())
}

#[async_trait]
impl BrowserBackend for ManagedBackend {
    async fn navigate(&self, url: &str) -> Result<String, BrowserError> {
        let page = self.page.lock().await;
        let nav = page.goto(url);
        match tokio::time::timeout(NAV_TIMEOUT, nav).await {
            Ok(r) => {
                r.map_err(|e| BrowserError::Protocol(e.to_string()))?;
            }
            Err(_) => { /* load timeout: return current state, not an error */ }
        }
        tokio::time::timeout(OP_TIMEOUT, page.url())
            .await
            .map_err(|_| BrowserError::Timeout("navigate: url".into()))?
            .map_err(|e| BrowserError::Protocol(e.to_string()))?
            .ok_or_else(|| BrowserError::Protocol("no url after navigation".into()))
    }

    async fn screenshot(&self) -> Result<Screenshot, BrowserError> {
        use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
        use chromiumoxide::page::ScreenshotParams;
        let page = self.page.lock().await;
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Jpeg)
            .quality(70)
            .full_page(false)
            .build();
        let bytes = tokio::time::timeout(OP_TIMEOUT, page.screenshot(params))
            .await
            .map_err(|_| BrowserError::Timeout("screenshot".into()))?
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;
        Ok(Screenshot {
            bytes,
            media_type: "image/jpeg",
        })
    }

    async fn snapshot(&self) -> Result<String, BrowserError> {
        let v = self.evaluate(SNAPSHOT_JS).await?;
        let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
        *self.snapshot_refs.lock().unwrap() = count;
        Ok(v.get("outline")
            .and_then(|o| o.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn click(&self, target: &ClickTarget) -> Result<(), BrowserError> {
        let page = self.page.lock().await;
        match target {
            ClickTarget::Selector(sel) => {
                let el = page
                    .find_element(sel.as_str())
                    .await
                    .map_err(|e| BrowserError::Protocol(format!("selector {sel:?}: {e}")))?;
                el.click()
                    .await
                    .map_err(|e| BrowserError::Protocol(e.to_string()))?;
            }
            ClickTarget::Ref(n) => {
                let max = *self.snapshot_refs.lock().unwrap();
                if *n == 0 || *n > max {
                    return Err(BrowserError::Protocol(format!(
                        "ref {n} out of range (run browser_read snapshot first; {max} refs)"
                    )));
                }
                let sel = format!("[data-zode-ref=\"{n}\"]");
                let el = page
                    .find_element(sel.as_str())
                    .await
                    .map_err(|e| BrowserError::Protocol(format!("stale ref {n}: {e}")))?;
                el.click()
                    .await
                    .map_err(|e| BrowserError::Protocol(e.to_string()))?;
            }
            ClickTarget::Coords { x, y } => {
                page.click(Point { x: *x, y: *y })
                    .await
                    .map_err(|e| BrowserError::Protocol(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn type_text(&self, target: &ClickTarget, text: &str) -> Result<(), BrowserError> {
        // Click first to focus the target, then dispatch keystrokes; CDP key
        // events go to whatever is currently focused, not a specific
        // element, so typing is page-scoped rather than target-specific.
        self.click(target).await?;
        let page = self.page.lock().await;
        dispatch_type_str(&page, text).await
    }

    async fn press_key(&self, key: &str) -> Result<(), BrowserError> {
        let page = self.page.lock().await;
        dispatch_key(&page, key).await
    }

    async fn scroll(&self, dx: f64, dy: f64) -> Result<(), BrowserError> {
        self.evaluate(&format!("window.scrollBy({dx},{dy})"))
            .await
            .map(|_| ())
    }

    async fn evaluate(&self, expression: &str) -> Result<serde_json::Value, BrowserError> {
        let page = self.page.lock().await;
        let res = tokio::time::timeout(OP_TIMEOUT, page.evaluate(expression))
            .await
            .map_err(|_| BrowserError::Timeout("evaluate".into()))?
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;
        Ok(res.value().cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn console_logs(&self, limit: usize) -> Result<Vec<ConsoleEntry>, BrowserError> {
        let buf = self.console_buf.lock().unwrap();
        Ok(buf.iter().rev().take(limit).rev().cloned().collect())
    }

    async fn network_log(&self, limit: usize) -> Result<Vec<NetworkEntry>, BrowserError> {
        let buf = self.network_buf.lock().unwrap();
        Ok(buf.iter().rev().take(limit).rev().cloned().collect())
    }

    async fn downloads(&self, limit: usize) -> Result<Vec<DownloadEntry>, BrowserError> {
        Ok(self.downloads.lock().unwrap().list(limit))
    }

    async fn set_file_input(
        &self,
        target: &ClickTarget,
        paths: &[PathBuf],
    ) -> Result<(), BrowserError> {
        let expression = super::file_input::expression(target, paths.len() > 1)?;
        let page = self.page.lock().await;
        let evaluated = page
            .execute(
                EvaluateParams::builder()
                    .expression(expression)
                    .return_by_value(false)
                    .build()
                    .map_err(BrowserError::Protocol)?,
            )
            .await
            .map_err(|e| BrowserError::Protocol(format!("file input evaluate: {e}")))?;
        if let Some(details) = evaluated.result.exception_details {
            let message = details
                .exception
                .and_then(|exception| exception.description)
                .unwrap_or(details.text);
            return Err(BrowserError::Protocol(format!(
                "file input evaluate: {}",
                message
            )));
        }
        let object_id = evaluated.result.result.object_id.ok_or_else(|| {
            BrowserError::Protocol("file input evaluate returned no objectId".into())
        })?;
        let files = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let set_result = page
            .execute(
                SetFileInputFilesParams::builder()
                    .files(files)
                    .object_id(object_id.clone())
                    .build()
                    .map_err(BrowserError::Protocol)?,
            )
            .await
            .map_err(|e| BrowserError::Protocol(format!("set file input: {e}")));
        let release_result = page.execute(ReleaseObjectParams::new(object_id)).await;
        set_result?;
        release_result
            .map_err(|e| BrowserError::Protocol(format!("release file input object: {e}")))?;
        Ok(())
    }

    async fn tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
        let pages = {
            let browser = self.browser.lock().await;
            browser
                .pages()
                .await
                .map_err(|e| BrowserError::Protocol(e.to_string()))?
        };
        let current_id = self.page.lock().await.target_id().clone();
        let mut out = Vec::with_capacity(pages.len());
        for p in pages {
            let url = p.url().await.ok().flatten().unwrap_or_default();
            let title = p.get_title().await.ok().flatten().unwrap_or_default();
            let active = *p.target_id() == current_id;
            out.push(TabInfo {
                id: p.target_id().as_ref().to_string(),
                url,
                title,
                active,
            });
        }
        Ok(out)
    }

    async fn tab_new(&self, url: Option<&str>) -> Result<TabInfo, BrowserError> {
        let new_page = {
            let browser = self.browser.lock().await;
            browser
                .new_page(url.unwrap_or("about:blank"))
                .await
                .map_err(|e| BrowserError::Launch(e.to_string()))?
        };
        let info = TabInfo {
            id: new_page.target_id().as_ref().to_string(),
            url: new_page.url().await.ok().flatten().unwrap_or_default(),
            title: new_page
                .get_title()
                .await
                .ok()
                .flatten()
                .unwrap_or_default(),
            active: true,
        };
        self.replace_listeners(&new_page);
        *self.page.lock().await = new_page;
        Ok(info)
    }

    async fn tab_close(&self, id: &str) -> Result<(), BrowserError> {
        let pages = {
            let browser = self.browser.lock().await;
            browser
                .pages()
                .await
                .map_err(|e| BrowserError::Protocol(e.to_string()))?
        };
        if pages.len() <= 1 {
            return Err(BrowserError::Protocol("cannot close the last tab".into()));
        }
        let target = pages
            .into_iter()
            .find(|p| p.target_id().as_ref() == id)
            .ok_or_else(|| BrowserError::Protocol(format!("no such tab: {id}")))?;
        let closed_id = target.target_id().clone();
        target
            .close()
            .await
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;

        // If the closed tab was the current one, fall back to whatever tab
        // remains so `self.page` never points at a dead target.
        let current_id = self.page.lock().await.target_id().clone();
        if current_id == closed_id {
            let remaining = {
                let browser = self.browser.lock().await;
                browser
                    .pages()
                    .await
                    .map_err(|e| BrowserError::Protocol(e.to_string()))?
            };
            if let Some(next) = remaining.into_iter().next() {
                self.replace_listeners(&next);
                *self.page.lock().await = next;
            }
        }
        Ok(())
    }

    async fn tab_select(&self, id: &str) -> Result<(), BrowserError> {
        let pages = {
            let browser = self.browser.lock().await;
            browser
                .pages()
                .await
                .map_err(|e| BrowserError::Protocol(e.to_string()))?
        };
        let target = pages
            .into_iter()
            .find(|p| p.target_id().as_ref() == id)
            .ok_or_else(|| BrowserError::Protocol(format!("no such tab: {id}")))?;
        target
            .bring_to_front()
            .await
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;
        self.replace_listeners(&target);
        *self.page.lock().await = target;
        Ok(())
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        let page = self.page.lock().await;
        tokio::time::timeout(OP_TIMEOUT, page.url())
            .await
            .map_err(|_| BrowserError::Timeout("current_url".into()))?
            .map_err(|e| BrowserError::Protocol(e.to_string()))?
            .ok_or_else(|| BrowserError::Protocol("no current url".into()))
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn close(&self) -> Result<(), BrowserError> {
        let mut browser = self.browser.lock().await;
        browser
            .close()
            .await
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;
        // Reap the spawned child so we don't leave a zombie process behind;
        // the supervisor task flips `alive` to false once the handler
        // stream ends as a side effect of the connection closing.
        let _ = browser.wait().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_dir_defaults_under_zode_home() {
        let cfg = BrowserConfig::default();
        let p = resolve_profile_dir(&cfg);
        assert!(p.ends_with("browser-profile"), "{p:?}");
    }
}
