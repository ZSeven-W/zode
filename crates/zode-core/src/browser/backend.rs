use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fmt;
use std::path::PathBuf;

/// Which browser instance to control.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserTarget {
    /// Process-wide singleton, chromiumoxide-backed.
    Managed,
    /// External OpenPencil bridge.
    Bridge,
}

/// Target for click/type operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ClickTarget {
    /// CSS selector.
    Selector(String),
    /// Element reference ID.
    Ref(u32),
    /// Absolute coordinates.
    Coords { x: f64, y: f64 },
}

/// Information about an open browser tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub active: bool,
}

/// A single console log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
}

/// A single network request/response entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    InProgress,
    Complete,
    Canceled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub status: DownloadStatus,
    pub path: Option<PathBuf>,
    pub url: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

/// A screenshot image.
#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}

/// One decoded-ready frame from a live browser frame stream (M1: JPEG only,
/// via CDP `Page.startScreencast`). `sequence` increases by one per frame
/// so callers (the desktop spectator panel) can detect a new frame arrived
/// without content-comparing bytes, and can mint a fresh cache key per
/// frame — reusing one key across frames would leave image caches keyed by
/// content identity permanently stuck on the first frame.
#[derive(Clone)]
pub struct ScreencastFrame {
    pub data: std::sync::Arc<Vec<u8>>,
    pub sequence: u64,
}

impl fmt::Debug for ScreencastFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScreencastFrame")
            .field("bytes", &self.data.len())
            .field("sequence", &self.sequence)
            .finish()
    }
}

/// Browser operation errors.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserError {
    NotFound(String),
    Launch(String),
    Protocol(String),
    Timeout(String),
    Dead(String),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrowserError::NotFound(msg) => write!(f, "browser executable not found: {}", msg),
            BrowserError::Launch(msg) => write!(f, "browser launch failed: {}", msg),
            BrowserError::Protocol(msg) => write!(f, "browser protocol error: {}", msg),
            BrowserError::Timeout(msg) => write!(f, "browser operation timed out: {}", msg),
            BrowserError::Dead(msg) => write!(f, "browser is not running: {}", msg),
        }
    }
}

impl std::error::Error for BrowserError {}

/// Backend for browser automation operations.
///
/// Implementers must be object-safe (`Send + Sync + Debug`).
#[async_trait]
pub trait BrowserBackend: Send + Sync + std::fmt::Debug {
    /// Navigate to a URL; returns the final URL (may differ after redirects).
    async fn navigate(&self, url: &str) -> Result<String, BrowserError>;

    /// Take a screenshot of the current tab.
    async fn screenshot(&self) -> Result<Screenshot, BrowserError>;

    /// Get the accessibility tree (snapshot) of the current tab.
    async fn snapshot(&self) -> Result<String, BrowserError>;

    /// Click a target on the page.
    async fn click(&self, target: &ClickTarget) -> Result<(), BrowserError>;

    /// Type text into a focused or targeted field.
    async fn type_text(&self, target: &ClickTarget, text: &str) -> Result<(), BrowserError>;

    /// Press a key (e.g., "Enter", "Escape", "Tab").
    async fn press_key(&self, key: &str) -> Result<(), BrowserError>;

    /// Scroll the page by dx, dy pixels.
    async fn scroll(&self, dx: f64, dy: f64) -> Result<(), BrowserError>;

    /// Evaluate a JavaScript expression in the current tab.
    async fn evaluate(&self, expression: &str) -> Result<serde_json::Value, BrowserError>;

    /// Get recent console log entries (up to limit).
    async fn console_logs(&self, limit: usize) -> Result<Vec<ConsoleEntry>, BrowserError>;

    /// Get recent network request/response entries (up to limit).
    async fn network_log(&self, limit: usize) -> Result<Vec<NetworkEntry>, BrowserError>;

    /// Get downloads observed during this backend's current session.
    async fn downloads(&self, limit: usize) -> Result<Vec<DownloadEntry>, BrowserError>;

    /// Set local files on a page `<input type="file">` element.
    async fn set_file_input(
        &self,
        target: &ClickTarget,
        paths: &[PathBuf],
    ) -> Result<(), BrowserError>;

    /// List all open tabs.
    async fn tabs(&self) -> Result<Vec<TabInfo>, BrowserError>;

    /// Open a new tab, optionally navigating to a URL.
    async fn tab_new(&self, url: Option<&str>) -> Result<TabInfo, BrowserError>;

    /// Close a tab by ID.
    async fn tab_close(&self, id: &str) -> Result<(), BrowserError>;

    /// Activate a tab by ID (bring it to focus).
    async fn tab_select(&self, id: &str) -> Result<(), BrowserError>;

    /// Get the URL of the current tab.
    async fn current_url(&self) -> Result<String, BrowserError>;

    /// Check if the browser instance is still alive.
    async fn is_alive(&self) -> bool;

    /// Whether this backend talks to the external bridge instead of the managed browser.
    fn is_bridge(&self) -> bool {
        false
    }

    /// Whether this backend can stream live frames (see `latest_frame`). The
    /// bridge backend has no screencast story in M1 — callers must check
    /// this before `start_frame_stream` and show an "unsupported" state
    /// instead of a permanently-idle one.
    fn supports_frame_stream(&self) -> bool {
        false
    }

    /// Starts (or restarts, e.g. after a panel resize) a live JPEG frame
    /// stream capped to `max_width` physical pixels. Backends that don't
    /// support streaming (see `supports_frame_stream`) return an error.
    async fn start_frame_stream(&self, max_width: u32) -> Result<(), BrowserError> {
        let _ = max_width;
        Err(BrowserError::Protocol(
            "frame streaming not supported by this backend".into(),
        ))
    }

    /// Stops a live frame stream started by `start_frame_stream`. Safe to
    /// call when no stream is active.
    async fn stop_frame_stream(&self) -> Result<(), BrowserError> {
        Ok(())
    }

    /// Non-blocking accessor for the most recently decoded-ready frame.
    /// `None` before the first frame arrives, after `stop_frame_stream`, or
    /// on a backend that doesn't support streaming. Unlike the other
    /// browser operations this never requires the session lease: the
    /// spectator panel polls it on every redraw tick and must never
    /// contend with an in-flight agent tool call.
    fn latest_frame(&self) -> Option<ScreencastFrame> {
        None
    }

    /// Close the browser instance.
    async fn close(&self) -> Result<(), BrowserError>;
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Records calls; every op succeeds with a canned value.
    #[derive(Debug, Default)]
    pub struct MockBackend {
        pub calls: AtomicUsize,
        pub dead: AtomicBool,
        pub in_flight: AtomicUsize,
        pub overlap_seen: AtomicBool,
        /// Set to make `supports_frame_stream` / `start_frame_stream` behave
        /// like `ManagedBackend`, so session-layer frame plumbing can be
        /// tested without a real browser.
        pub frame_stream_supported: AtomicBool,
        pub frame_stream_started: AtomicBool,
        pub frame: std::sync::Mutex<Option<ScreencastFrame>>,
    }

    impl MockBackend {
        /// Injects a frame as if a live stream produced it, incrementing
        /// `sequence` past whatever was there before.
        pub fn push_frame(&self, bytes: Vec<u8>) {
            let mut slot = self.frame.lock().unwrap();
            let sequence = slot.as_ref().map_or(0, |f| f.sequence) + 1;
            *slot = Some(ScreencastFrame {
                data: Arc::new(bytes),
                sequence,
            });
        }
    }

    impl MockBackend {
        async fn track<T>(&self, v: T) -> T {
            // Detect overlapping ops: session must serialize us.
            if self.in_flight.fetch_add(1, Ordering::SeqCst) > 0 {
                self.overlap_seen.store(true, Ordering::SeqCst);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            self.calls.fetch_add(1, Ordering::SeqCst);
            v
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for MockBackend {
        async fn navigate(&self, url: &str) -> Result<String, BrowserError> {
            self.track(Ok(url.to_string())).await
        }
        async fn screenshot(&self) -> Result<Screenshot, BrowserError> {
            self.track(Ok(Screenshot {
                bytes: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1],
                media_type: "image/png",
            }))
            .await
        }
        async fn snapshot(&self) -> Result<String, BrowserError> {
            self.track(Ok("[1] <body>".into())).await
        }
        async fn click(&self, _t: &ClickTarget) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn type_text(&self, _t: &ClickTarget, _s: &str) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn press_key(&self, _k: &str) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn scroll(&self, _dx: f64, _dy: f64) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn evaluate(&self, _e: &str) -> Result<serde_json::Value, BrowserError> {
            self.track(Ok(serde_json::json!(2))).await
        }
        async fn console_logs(&self, _n: usize) -> Result<Vec<ConsoleEntry>, BrowserError> {
            self.track(Ok(vec![ConsoleEntry {
                level: "log".into(),
                text: "hi".into(),
            }]))
            .await
        }
        async fn network_log(&self, _n: usize) -> Result<Vec<NetworkEntry>, BrowserError> {
            self.track(Ok(vec![])).await
        }
        async fn downloads(&self, n: usize) -> Result<Vec<DownloadEntry>, BrowserError> {
            self.track(Ok(vec![DownloadEntry {
                status: DownloadStatus::Complete,
                path: Some(std::path::PathBuf::from("/tmp/file.txt")),
                url: format!("https://example.test/file-{n}.txt"),
                received_bytes: 4,
                total_bytes: 4,
                error: None,
                attribution: None,
            }]))
            .await
        }
        async fn set_file_input(
            &self,
            _target: &ClickTarget,
            paths: &[PathBuf],
        ) -> Result<(), BrowserError> {
            assert!(paths.iter().all(|path| path.is_absolute()));
            self.track(Ok(())).await
        }
        async fn tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
            self.track(Ok(vec![TabInfo {
                id: "t1".into(),
                url: "about:blank".into(),
                title: "tab".into(),
                active: true,
            }]))
            .await
        }
        async fn tab_new(&self, _u: Option<&str>) -> Result<TabInfo, BrowserError> {
            self.track(Ok(TabInfo {
                id: "t2".into(),
                url: "about:blank".into(),
                title: "new".into(),
                active: true,
            }))
            .await
        }
        async fn tab_close(&self, _id: &str) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn tab_select(&self, _id: &str) -> Result<(), BrowserError> {
            self.track(Ok(())).await
        }
        async fn current_url(&self) -> Result<String, BrowserError> {
            self.track(Ok("https://example.test/".into())).await
        }
        async fn is_alive(&self) -> bool {
            !self.dead.load(Ordering::SeqCst)
        }
        async fn close(&self) -> Result<(), BrowserError> {
            Ok(())
        }
        fn supports_frame_stream(&self) -> bool {
            self.frame_stream_supported.load(Ordering::SeqCst)
        }
        async fn start_frame_stream(&self, _max_width: u32) -> Result<(), BrowserError> {
            if !self.supports_frame_stream() {
                return Err(BrowserError::Protocol(
                    "frame streaming not supported by this backend".into(),
                ));
            }
            self.frame_stream_started.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn stop_frame_stream(&self) -> Result<(), BrowserError> {
            self.frame_stream_started.store(false, Ordering::SeqCst);
            Ok(())
        }
        fn latest_frame(&self) -> Option<ScreencastFrame> {
            if !self.frame_stream_started.load(Ordering::SeqCst) {
                return None;
            }
            self.frame.lock().unwrap().clone()
        }
    }

    /// Factory that always returns a fresh `MockBackend`, tracking creation
    /// count and exposing the most recently created backend for assertions.
    #[derive(Debug)]
    pub(crate) struct MockFactory {
        pub(crate) made: AtomicUsize,
        pub(crate) current: std::sync::Mutex<Option<Arc<MockBackend>>>,
    }

    impl MockFactory {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                made: 0.into(),
                current: std::sync::Mutex::new(None),
            })
        }
    }

    #[async_trait::async_trait]
    impl crate::browser::session::BackendFactory for MockFactory {
        async fn create(
            &self,
            _cfg: &crate::config::BrowserConfig,
        ) -> Result<Arc<dyn BrowserBackend>, BrowserError> {
            self.made.fetch_add(1, Ordering::SeqCst);
            let b = Arc::new(MockBackend::default());
            *self.current.lock().unwrap() = Some(b.clone());
            Ok(b)
        }
    }

    /// Ready-made `BackendFactory` for tests that just need a working mock
    /// session and don't care about call-count assertions.
    pub(crate) fn mock_factory() -> Arc<dyn crate::browser::session::BackendFactory> {
        MockFactory::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_prefixed() {
        assert_eq!(
            BrowserError::Launch("no chrome".into()).to_string(),
            "browser launch failed: no chrome"
        );
        assert_eq!(
            BrowserError::Dead("gone".into()).to_string(),
            "browser is not running: gone"
        );
    }

    #[tokio::test]
    async fn mock_backend_is_object_safe() {
        let b: std::sync::Arc<dyn BrowserBackend> =
            std::sync::Arc::new(mock::MockBackend::default());
        assert_eq!(b.evaluate("1+1").await.unwrap(), serde_json::json!(2));
    }
}
