//! Test-only mock DesktopBackend + factory, mirroring browser::backend::mock.
//! All tasks 1–11 are provable against this — no real AX/Chrome required.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::backend::*;

#[derive(Debug)]
pub struct MockDesktopBackend {
    pub calls: AtomicUsize,
    alive: AtomicBool,
}

impl MockDesktopBackend {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            alive: AtomicBool::new(true),
        })
    }
    pub fn set_alive(&self, v: bool) {
        self.alive.store(v, Ordering::SeqCst);
    }
}

#[async_trait]
impl DesktopBackend for MockDesktopBackend {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![AppInfo {
            name: "TextEdit".into(),
            executable_identity: "com.apple.TextEdit".into(),
            is_electron: false,
        }])
    }
    async fn list_windows(&self, _app: &AppId) -> Result<Vec<WindowInfo>, DesktopError> {
        Ok(vec![WindowInfo {
            token: "w1".into(),
            title: Some("Untitled".into()),
        }])
    }
    async fn snapshot(
        &self,
        _win: &WindowId,
        _scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError> {
        Ok(SnapshotResult {
            outline: "[e1] Button \"Save\"".into(),
            snapshot_generation: 1,
        })
    }
    async fn element_action(
        &self,
        _r: &ElementRef,
        _kind: ElementActionKind,
    ) -> Result<String, DesktopError> {
        Ok("ok".into())
    }
    async fn set_value(&self, _r: &ElementRef, _text: &str) -> Result<(), DesktopError> {
        Ok(())
    }
    async fn type_text(&self, _win: &WindowId, _text: &str) -> Result<(), DesktopError> {
        Ok(())
    }
    async fn key(&self, _win: &WindowId, _combo: &str) -> Result<(), DesktopError> {
        Ok(())
    }
    async fn focus_window(&self, _win: &WindowId) -> Result<(), DesktopError> {
        Ok(())
    }
    async fn launch_app(&self, _ident: &AppLaunchId) -> Result<AppInfo, DesktopError> {
        Ok(AppInfo {
            name: "TextEdit".into(),
            executable_identity: "com.apple.TextEdit".into(),
            is_electron: false,
        })
    }
    async fn screenshot(&self, _win: &WindowId) -> Result<Screenshot, DesktopError> {
        // Minimal JPEG SOI+EOI markers; enough for the sentinel helper test.
        Ok(Screenshot {
            bytes: vec![0xFF, 0xD8, 0xFF, 0xD9],
            media_type: "image/jpeg",
        })
    }
    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    async fn close(&self) -> Result<(), DesktopError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MockDesktopFactory {
    pub made: AtomicUsize,
}

#[async_trait]
impl DesktopBackendFactory for MockDesktopFactory {
    async fn create(&self) -> Result<Arc<dyn DesktopBackend>, DesktopError> {
        self.made.fetch_add(1, Ordering::SeqCst);
        Ok(MockDesktopBackend::new())
    }
}

pub fn mock_factory() -> Arc<dyn DesktopBackendFactory> {
    Arc::new(MockDesktopFactory::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_snapshot_returns_outline_with_ref() {
        let b = MockDesktopBackend::new();
        let app = AppId::new(1, 100, "com.test".into(), 0);
        let win = WindowId::new(app, 1, 1, 0);
        let snap = b.snapshot(&win, None).await.unwrap();
        assert!(snap.outline.contains("[e1]"));
    }

    #[tokio::test]
    async fn mock_can_inject_dead() {
        let b = MockDesktopBackend::new();
        b.set_alive(false);
        assert!(!b.is_alive().await);
    }
}
