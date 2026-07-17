//! Linux AT-SPI2 desktop backend. AT-SPI2 is an async D-Bus accessibility bus;
//! its zbus proxies are `Send`/`Sync`, so (unlike the macOS AX / Windows UIA
//! actor-thread backends) this backend holds the connection directly and calls
//! proxies from async methods — no dedicated OS thread is needed.
//!
//! NOTE: written against the `atspi` 0.26 proxy API but built and verified only
//! on Linux (`#[cfg(target_os = "linux")]`). It could not be compiled on the
//! macOS development host — the exact proxy method names / `ObjectRef` shape may
//! need small adjustments; verify on real hardware.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use atspi::connection::AccessibilityConnection;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::ObjectRef;
use tokio::sync::Mutex;

use crate::desktop::backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopBackendFactory, DesktopError,
    ElementActionKind, ElementRef, Screenshot, SnapshotResult, WindowId, WindowInfo,
};

const ROOT_DEST: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";

#[derive(Debug, Default)]
pub struct AtspiFactory;

#[async_trait]
impl DesktopBackendFactory for AtspiFactory {
    async fn create(&self) -> Result<Arc<dyn DesktopBackend>, DesktopError> {
        let conn = AccessibilityConnection::new()
            .await
            .map_err(|e| DesktopError::Dead(format!("connect a11y bus: {e}")))?;
        Ok(Arc::new(AtspiBackend {
            conn,
            snapshots: Mutex::new(HashMap::new()),
        }))
    }
}

#[derive(Debug)]
pub struct AtspiBackend {
    conn: AccessibilityConnection,
    /// Latest snapshot's object refs per (app index, window index).
    snapshots: Mutex<HashMap<(usize, usize), Vec<ObjectRef>>>,
}

/// Encode an app index in the executable identity ("app#<index>").
fn parse_index(exe: &str) -> Option<usize> {
    exe.rsplit('#').next()?.parse().ok()
}

impl AtspiBackend {
    /// Build an AccessibleProxy for an object reference.
    async fn accessible<'a>(
        &'a self,
        obj: &ObjectRef,
    ) -> Result<AccessibleProxy<'a>, DesktopError> {
        AccessibleProxy::builder(self.conn.connection())
            .destination(obj.name.clone())
            .and_then(|b| b.path(obj.path.clone()))
            .map_err(|e| DesktopError::Protocol(format!("proxy addr: {e}")))?
            .build()
            .await
            .map_err(|e| DesktopError::Protocol(format!("build accessible proxy: {e}")))
    }

    /// The desktop root accessible (its children are the running apps).
    async fn root(&self) -> Result<AccessibleProxy<'_>, DesktopError> {
        AccessibleProxy::builder(self.conn.connection())
            .destination(ROOT_DEST)
            .and_then(|b| b.path(ROOT_PATH))
            .map_err(|e| DesktopError::Protocol(format!("root addr: {e}")))?
            .build()
            .await
            .map_err(|e| DesktopError::Protocol(format!("build root proxy: {e}")))
    }

    async fn children(&self, elem: &AccessibleProxy<'_>) -> Vec<ObjectRef> {
        let count = elem.child_count().await.unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..count {
            if let Ok(child) = elem.get_child_at_index(i).await {
                out.push(child);
            }
        }
        out
    }

    async fn app_refs(&self) -> Result<Vec<ObjectRef>, DesktopError> {
        let root = self.root().await?;
        Ok(self.children(&root).await)
    }
}

#[async_trait]
impl DesktopBackend for AtspiBackend {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError> {
        let apps = self.app_refs().await?;
        let mut out = Vec::new();
        for (i, obj) in apps.iter().enumerate() {
            let name = match self.accessible(obj).await {
                Ok(p) => p.name().await.unwrap_or_default(),
                Err(_) => String::new(),
            };
            out.push(AppInfo {
                name: if name.is_empty() {
                    format!("app {i}")
                } else {
                    name
                },
                executable_identity: format!("app#{i}"),
                is_electron: false,
            });
        }
        Ok(out)
    }

    async fn list_windows(&self, app: &AppId) -> Result<Vec<WindowInfo>, DesktopError> {
        let idx = parse_index(app.executable_identity())
            .ok_or_else(|| DesktopError::NotFound("app identity has no index".into()))?;
        let apps = self.app_refs().await?;
        let app_obj = apps
            .get(idx)
            .ok_or_else(|| DesktopError::NotFound("app index out of range".into()))?;
        let app_proxy = self.accessible(app_obj).await?;
        let windows = self.children(&app_proxy).await;
        let mut out = Vec::new();
        for (i, w) in windows.iter().enumerate() {
            let title = match self.accessible(w).await {
                Ok(p) => p.name().await.ok().filter(|s| !s.is_empty()),
                Err(_) => None,
            };
            out.push(WindowInfo {
                token: i.to_string(),
                title,
            });
        }
        Ok(out)
    }

    async fn snapshot(
        &self,
        win: &WindowId,
        _scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError> {
        let app_idx = parse_index(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no index".into()))?;
        let win_idx = win.actor_local_key() as usize;
        let apps = self.app_refs().await?;
        let app_obj = apps
            .get(app_idx)
            .ok_or_else(|| DesktopError::NotFound("app index out of range".into()))?;
        let app_proxy = self.accessible(app_obj).await?;
        let windows = self.children(&app_proxy).await;
        let win_obj = windows
            .get(win_idx)
            .ok_or_else(|| DesktopError::NotFound("window index out of range".into()))?
            .clone();

        let mut nodes: Vec<ObjectRef> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        self.walk(&win_obj, 0, 500, &mut nodes, &mut lines).await;
        self.snapshots
            .lock()
            .await
            .insert((app_idx, win_idx), nodes);
        Ok(SnapshotResult {
            outline: lines.join("\n"),
            snapshot_generation: 1,
        })
    }

    async fn element_action(
        &self,
        r: &ElementRef,
        _kind: ElementActionKind,
    ) -> Result<String, DesktopError> {
        let obj = self.node_ref(r).await?;
        // AT-SPI Action interface: invoke the first action ("click"/"activate").
        let action = atspi::proxy::action::ActionProxy::builder(self.conn.connection())
            .destination(obj.name.clone())
            .and_then(|b| b.path(obj.path.clone()))
            .map_err(|e| DesktopError::Protocol(format!("action addr: {e}")))?
            .build()
            .await
            .map_err(|_| {
                DesktopError::UnsupportedAction("element exposes no Action interface".into())
            })?;
        action
            .do_action(0)
            .await
            .map_err(|e| DesktopError::Protocol(format!("do_action: {e}")))?;
        Ok("ok:action".into())
    }

    async fn set_value(&self, r: &ElementRef, text: &str) -> Result<(), DesktopError> {
        let obj = self.node_ref(r).await?;
        // EditableText interface: replace the whole text.
        let editable =
            atspi::proxy::editable_text::EditableTextProxy::builder(self.conn.connection())
                .destination(obj.name.clone())
                .and_then(|b| b.path(obj.path.clone()))
                .map_err(|e| DesktopError::Protocol(format!("editable addr: {e}")))?
                .build()
                .await
                .map_err(|_| {
                    DesktopError::UnsupportedAction("element is not editable text".into())
                })?;
        editable
            .set_text_contents(text)
            .await
            .map_err(|e| DesktopError::Protocol(format!("set_text_contents: {e}")))?;
        Ok(())
    }

    async fn type_text(&self, _win: &WindowId, _text: &str) -> Result<(), DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "AT-SPI key synthesis (GenerateKeyboardEvent) is deferred; use set_value".into(),
        ))
    }

    async fn key(&self, _win: &WindowId, _combo: &str) -> Result<(), DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "AT-SPI key combos are deferred".into(),
        ))
    }

    async fn focus_window(&self, _win: &WindowId) -> Result<(), DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "AT-SPI window focus is deferred".into(),
        ))
    }

    async fn launch_app(&self, _ident: &AppLaunchId) -> Result<AppInfo, DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "launch is not an AT-SPI operation".into(),
        ))
    }

    async fn screenshot(&self, _win: &WindowId) -> Result<Screenshot, DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "AT-SPI has no screen capture; use a compositor screenshot API".into(),
        ))
    }

    async fn is_alive(&self) -> bool {
        self.root().await.is_ok()
    }

    async fn close(&self) -> Result<(), DesktopError> {
        Ok(())
    }
}

impl AtspiBackend {
    async fn walk(
        &self,
        obj: &ObjectRef,
        depth: usize,
        max_nodes: usize,
        nodes: &mut Vec<ObjectRef>,
        lines: &mut Vec<String>,
    ) {
        if nodes.len() >= max_nodes {
            return;
        }
        let (role, name) = match self.accessible(obj).await {
            Ok(p) => (
                p.get_localized_role_name()
                    .await
                    .unwrap_or_else(|_| "object".into()),
                p.name().await.unwrap_or_default(),
            ),
            Err(_) => ("(unreadable)".into(), String::new()),
        };
        nodes.push(obj.clone());
        let n = nodes.len();
        let indent = "  ".repeat(depth);
        lines.push(if name.is_empty() {
            format!("{indent}[e{n}] {role}")
        } else {
            format!("{indent}[e{n}] {role} {name:?}")
        });

        if let Ok(p) = self.accessible(obj).await {
            for child in self.children(&p).await {
                if nodes.len() >= max_nodes {
                    break;
                }
                Box::pin(self.walk(&child, depth + 1, max_nodes, nodes, lines)).await;
            }
        }
    }

    async fn node_ref(&self, r: &ElementRef) -> Result<ObjectRef, DesktopError> {
        let app_idx = parse_index(r.window().app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("ref app has no index".into()))?;
        let win_idx = r.window().actor_local_key() as usize;
        let map = self.snapshots.lock().await;
        let nodes = map
            .get(&(app_idx, win_idx))
            .ok_or_else(|| DesktopError::StaleRef {
                reason: "no snapshot for this window; run snapshot first".into(),
            })?;
        (r.local_id() as usize)
            .checked_sub(1)
            .and_then(|i| nodes.get(i))
            .cloned()
            .ok_or_else(|| DesktopError::StaleRef {
                reason: format!("ref e{} not in current snapshot", r.local_id()),
            })
    }
}
