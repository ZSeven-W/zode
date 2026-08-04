//! CDP desktop backend: attaches to an Electron/Chromium instance's DevTools
//! debug port and drives its page targets as "windows". Reuses the browser
//! subsystem's `SNAPSHOT_JS` so the model sees the same ref-annotated outline
//! format as native AX snapshots (spec §Electron → CDP). Verifiable against any
//! Chromium launched with `--remote-debugging-port`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::target::TargetId;
use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::Mutex;

use crate::browser::snapshot_js::SNAPSHOT_JS;
use crate::desktop::backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopError, ElementActionKind, ElementRef,
    Screenshot, SnapshotResult, WindowId, WindowInfo,
};

const OP_TIMEOUT: Duration = Duration::from_secs(15);

/// Detect whether a macOS app bundle / Windows install dir looks like Electron.
/// A candidate signal only (spec: fingerprints never auto-trigger anything);
/// final confirmation is the CDP `/json/version` product string.
pub fn looks_like_electron(app_dir: &std::path::Path) -> bool {
    // macOS: <bundle>/Contents/Frameworks/Electron Framework.framework
    let mac = app_dir
        .join("Contents/Frameworks/Electron Framework.framework")
        .exists();
    // Windows/Linux layout: v8 snapshot + chromium pak next to the exe.
    let generic = app_dir.join("v8_context_snapshot.bin").exists()
        || app_dir.join("resources/electron.asar").exists();
    mac || generic
}

/// A CDP-attached instance. Holds the chromiumoxide `Browser` and a supervisor
/// task that continuously drives the handler stream (required — without it the
/// connection does nothing, per the browser subsystem's managed backend).
#[derive(Debug)]
pub struct CdpBackend {
    browser: Mutex<Browser>,
    /// Page target ids discovered on the last `list_windows`, indexed by the
    /// token the model uses (`0`, `1`, …).
    page_targets: Mutex<Vec<TargetId>>,
    product: String,
    port: u16,
    _supervisor: tokio::task::JoinHandle<()>,
}

impl CdpBackend {
    /// Attach to `http://127.0.0.1:<port>` (loopback only). `connect` resolves
    /// the websocket URL from `/json/version` itself.
    pub async fn attach(port: u16) -> Result<Arc<Self>, DesktopError> {
        let url = format!("http://127.0.0.1:{port}");
        let (browser, mut handler) = tokio::time::timeout(OP_TIMEOUT, Browser::connect(url))
            .await
            .map_err(|_| DesktopError::Timeout("cdp connect".into()))?
            .map_err(|e| DesktopError::Protocol(format!("cdp connect: {e}")))?;
        // Supervisor: the handler stream MUST be polled continuously.
        let supervisor = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let product = browser
            .version()
            .await
            .map(|v| v.product)
            .unwrap_or_else(|_| "unknown".to_string());
        Ok(Arc::new(Self {
            browser: Mutex::new(browser),
            page_targets: Mutex::new(Vec::new()),
            product,
            port,
            _supervisor: supervisor,
        }))
    }

    /// Discover current `page` targets (order stable within a call); caches
    /// their ids so later actions resolve a model token → target.
    async fn refresh_targets(&self) -> Result<Vec<(TargetId, String)>, DesktopError> {
        let mut browser = self.browser.lock().await;
        let targets = browser
            .fetch_targets()
            .await
            .map_err(|e| DesktopError::Protocol(format!("cdp fetch_targets: {e}")))?;
        let pages: Vec<(TargetId, String)> = targets
            .into_iter()
            .filter(|t| t.r#type == "page")
            .map(|t| (t.target_id, t.title))
            .collect();
        *self.page_targets.lock().await = pages.iter().map(|(id, _)| id.clone()).collect();
        Ok(pages)
    }

    async fn page_at(&self, index: usize) -> Result<Page, DesktopError> {
        let id = {
            let ids = self.page_targets.lock().await;
            ids.get(index).cloned()
        };
        // If the model acts before listing, discover targets first.
        let id = match id {
            Some(id) => id,
            None => self
                .refresh_targets()
                .await?
                .into_iter()
                .nth(index)
                .map(|(id, _)| id)
                .ok_or_else(|| DesktopError::NotFound(format!("no cdp page at index {index}")))?,
        };
        // fetch_targets triggers an async AttachToTarget; the page only enters
        // the handler registry once that round-trip completes. Retry briefly.
        let mut last = DesktopError::NotFound("cdp page not attached yet".into());
        for _ in 0..30 {
            {
                let browser = self.browser.lock().await;
                match browser.get_page(id.clone()).await {
                    Ok(p) => return Ok(p),
                    Err(e) => last = DesktopError::Protocol(format!("cdp get_page: {e}")),
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(last)
    }

    async fn eval_on(&self, index: usize, expr: &str) -> Result<serde_json::Value, DesktopError> {
        let page = self.page_at(index).await?;
        let res = tokio::time::timeout(OP_TIMEOUT, page.evaluate(expr))
            .await
            .map_err(|_| DesktopError::Timeout("cdp evaluate".into()))?
            .map_err(|e| DesktopError::Protocol(format!("cdp evaluate: {e}")))?;
        Ok(res.value().cloned().unwrap_or(serde_json::Value::Null))
    }
}

/// Escape a string for embedding inside a JS single-quoted literal.
fn js_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
}

#[async_trait]
impl DesktopBackend for CdpBackend {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError> {
        Ok(vec![AppInfo {
            name: self.product.clone(),
            executable_identity: format!("cdp#{}", self.port),
            is_electron: true,
        }])
    }

    async fn list_windows(&self, _app: &AppId) -> Result<Vec<WindowInfo>, DesktopError> {
        let pages = self.refresh_targets().await?;
        Ok(pages
            .into_iter()
            .enumerate()
            .map(|(i, (_id, title))| WindowInfo {
                token: i.to_string(),
                title: (!title.is_empty()).then_some(title),
            })
            .collect())
    }

    async fn snapshot(
        &self,
        win: &WindowId,
        _scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError> {
        let index = win.actor_local_key() as usize;
        let v = self.eval_on(index, SNAPSHOT_JS).await?;
        let outline = v
            .get("outline")
            .and_then(|o| o.as_str())
            .unwrap_or("")
            .to_string();
        Ok(SnapshotResult {
            outline,
            snapshot_generation: 1,
        })
    }

    async fn element_action(
        &self,
        r: &ElementRef,
        kind: ElementActionKind,
    ) -> Result<String, DesktopError> {
        let index = r.window().actor_local_key() as usize;
        let sel = format!("[data-zode-ref=\"{}\"]", r.local_id());
        let expr = match kind {
            ElementActionKind::Click | ElementActionKind::Toggle | ElementActionKind::Expand => {
                format!("(() => {{ const e=document.querySelector('{sel}'); if(!e) return 'missing'; e.click(); return 'ok'; }})()")
            }
            ElementActionKind::Scroll => format!(
                "(() => {{ const e=document.querySelector('{sel}'); if(!e) return 'missing'; e.scrollIntoView(); return 'ok'; }})()"
            ),
        };
        let v = self.eval_on(index, &expr).await?;
        match v.as_str() {
            Some("ok") => Ok("ok".into()),
            _ => Err(DesktopError::StaleRef {
                reason: format!("ref e{} not found in page", r.local_id()),
            }),
        }
    }

    async fn set_value(&self, r: &ElementRef, text: &str) -> Result<(), DesktopError> {
        let index = r.window().actor_local_key() as usize;
        let sel = format!("[data-zode-ref=\"{}\"]", r.local_id());
        let expr = format!(
            "(() => {{ const e=document.querySelector('{sel}'); if(!e) return 'missing'; \
             e.value='{}'; e.dispatchEvent(new Event('input',{{bubbles:true}})); return 'ok'; }})()",
            js_str(text)
        );
        match self.eval_on(index, &expr).await?.as_str() {
            Some("ok") => Ok(()),
            _ => Err(DesktopError::StaleRef {
                reason: "element not found for set_value".into(),
            }),
        }
    }

    async fn type_text(&self, win: &WindowId, text: &str) -> Result<(), DesktopError> {
        // CDP path: insert into the active element via JS (M1 keyboard synthesis
        // is macOS-specific; for CDP we target the focused element directly).
        let index = win.actor_local_key() as usize;
        let expr = format!(
            "(() => {{ const e=document.activeElement; if(!e) return 'missing'; \
             if('value' in e) {{ e.value += '{t}'; e.dispatchEvent(new Event('input',{{bubbles:true}})); }} \
             return 'ok'; }})()",
            t = js_str(text)
        );
        self.eval_on(index, &expr).await?;
        Ok(())
    }

    async fn key(&self, _win: &WindowId, _combo: &str) -> Result<(), DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "key combos over CDP are deferred; use element actions or type".into(),
        ))
    }

    async fn focus_window(&self, win: &WindowId) -> Result<(), DesktopError> {
        let index = win.actor_local_key() as usize;
        let page = self.page_at(index).await?;
        page.bring_to_front()
            .await
            .map_err(|e| DesktopError::Protocol(format!("cdp bring_to_front: {e}")))?;
        Ok(())
    }

    async fn launch_app(&self, _ident: &AppLaunchId) -> Result<AppInfo, DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "launch is not a CDP operation".into(),
        ))
    }

    async fn screenshot(&self, win: &WindowId) -> Result<Screenshot, DesktopError> {
        use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
        use chromiumoxide::page::ScreenshotParams;
        let index = win.actor_local_key() as usize;
        let page = self.page_at(index).await?;
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .full_page(false)
            .build();
        let bytes = tokio::time::timeout(OP_TIMEOUT, page.screenshot(params))
            .await
            .map_err(|_| DesktopError::Timeout("cdp screenshot".into()))?
            .map_err(|e| DesktopError::Protocol(format!("cdp screenshot: {e}")))?;
        Ok(Screenshot {
            bytes,
            media_type: "image/png",
        })
    }

    async fn is_alive(&self) -> bool {
        self.refresh_targets().await.is_ok()
    }

    async fn close(&self) -> Result<(), DesktopError> {
        // Detach only — never kill the user's app.
        Ok(())
    }
}

/// Evaluate arbitrary JS on a CDP page (backs the `DesktopEval` tool).
impl CdpBackend {
    pub async fn evaluate(
        &self,
        win: &WindowId,
        expr: &str,
    ) -> Result<serde_json::Value, DesktopError> {
        let index = win.actor_local_key() as usize;
        self.eval_on(index, expr).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_fingerprint_detection() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!looks_like_electron(dir.path()));
        std::fs::create_dir_all(
            dir.path()
                .join("Contents/Frameworks/Electron Framework.framework"),
        )
        .unwrap();
        assert!(looks_like_electron(dir.path()));
    }

    #[test]
    fn js_str_escapes() {
        assert_eq!(js_str("a'b\\c\nd"), "a\\'b\\\\c\\nd");
    }
}
