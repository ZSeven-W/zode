//! Managed backend: zode-owned Chrome instance over CDP via
//! chromiumoxide, isolated persistent profile (Chrome 136+ forbids CDP
//! on the default profile — this is also the M1 login-state story).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::BrowserConfig as CdpBrowserConfig;
use chromiumoxide::Page;
use futures::StreamExt;

use crate::config::BrowserConfig;

use super::backend::*;
use super::session::BackendFactory;

const OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const NAV_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[allow(dead_code)] // consumed by console_logs/network_log once implemented (Task 7)
const LOG_BUFFER_CAP: usize = 500;

/// Candidate executable paths per platform, tried in order.
fn candidate_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
    #[cfg(target_os = "linux")]
    {
        [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
        ]
        .iter()
        .filter_map(|n| which_in_path(n))
        .collect()
    }
    #[cfg(target_os = "windows")]
    {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        [
            format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
            format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
}

#[cfg(target_os = "linux")]
fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// Explicit `browser.executable` wins when it points at a real file;
/// otherwise probe the platform's well-known install locations.
pub(crate) fn locate_executable(cfg: &BrowserConfig) -> Result<PathBuf, BrowserError> {
    if let Some(exe) = &cfg.executable {
        let p = PathBuf::from(exe);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(BrowserError::NotFound(format!(
                "configured browser.executable does not exist: {exe}"
            )))
        };
    }
    let tried = candidate_paths();
    tried.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        BrowserError::NotFound(format!(
            "no Chrome/Chromium/Edge found; tried {:?}; set browser.executable in config",
            tried
        ))
    })
}

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
    // Populated by Task 7's console/network event listeners and read by the
    // `console_logs`/`network_log` trait methods once those are implemented
    // (still `Err("implemented in task 7")` placeholders in this task).
    #[allow(dead_code)]
    console_buf: Arc<StdMutex<VecDeque<ConsoleEntry>>>,
    #[allow(dead_code)]
    network_buf: Arc<StdMutex<VecDeque<NetworkEntry>>>,
    // Last accessibility-snapshot ref counter, for click-by-ref validation
    // (Task 7).
    #[allow(dead_code)]
    snapshot_refs: StdMutex<u32>,
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
        let exe = locate_executable(cfg)?;
        let profile = resolve_profile_dir(cfg);
        std::fs::create_dir_all(&profile)
            .map_err(|e| BrowserError::Launch(format!("profile dir: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700));
        }
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
        let backend = Arc::new(Self {
            browser: tokio::sync::Mutex::new(browser),
            page: tokio::sync::Mutex::new(page),
            alive,
            console_buf: Arc::new(StdMutex::new(VecDeque::new())),
            network_buf: Arc::new(StdMutex::new(VecDeque::new())),
            snapshot_refs: StdMutex::new(0),
        });
        backend.spawn_log_listeners().await; // Task 7 fills this in
        Ok(backend)
    }

    async fn spawn_log_listeners(&self) {} // Task 7
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
        page.url()
            .await
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
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn click(&self, _target: &ClickTarget) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn type_text(&self, _target: &ClickTarget, _text: &str) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn press_key(&self, _key: &str) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn scroll(&self, _dx: f64, _dy: f64) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn evaluate(&self, expression: &str) -> Result<serde_json::Value, BrowserError> {
        let page = self.page.lock().await;
        let res = tokio::time::timeout(OP_TIMEOUT, page.evaluate(expression))
            .await
            .map_err(|_| BrowserError::Timeout("evaluate".into()))?
            .map_err(|e| BrowserError::Protocol(e.to_string()))?;
        Ok(res.value().cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn console_logs(&self, _limit: usize) -> Result<Vec<ConsoleEntry>, BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn network_log(&self, _limit: usize) -> Result<Vec<NetworkEntry>, BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn tab_new(&self, _url: Option<&str>) -> Result<TabInfo, BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn tab_close(&self, _id: &str) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn tab_select(&self, _id: &str) -> Result<(), BrowserError> {
        Err(BrowserError::Protocol("implemented in task 7".into()))
    }

    async fn current_url(&self) -> Result<String, BrowserError> {
        let page = self.page.lock().await;
        page.url()
            .await
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
    fn explicit_executable_wins() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("mychrome");
        std::fs::write(&fake, "").unwrap();
        let cfg = BrowserConfig {
            executable: Some(fake.to_string_lossy().into_owned()),
            ..Default::default()
        };
        assert_eq!(locate_executable(&cfg).unwrap(), fake);
    }

    #[test]
    fn missing_explicit_executable_errors_with_path() {
        let cfg = BrowserConfig {
            executable: Some("/definitely/not/here".into()),
            ..Default::default()
        };
        let err = locate_executable(&cfg).unwrap_err();
        assert!(err.to_string().contains("/definitely/not/here"));
    }

    #[test]
    fn profile_dir_defaults_under_zode_home() {
        let cfg = BrowserConfig::default();
        let p = resolve_profile_dir(&cfg);
        assert!(p.ends_with("browser-profile"), "{p:?}");
    }
}
