//! Browser spectator panel lifecycle (M1 route A - read-only, see
//! `docs/proposals/builtin-browser.md`). Mirrors `terminal.rs`'s shape:
//! open/close drive a live resource (a CDP screencast instead of a PTY),
//! and a per-tick poll drains whatever arrived into paint-ready state.
//!
//! Unlike the terminal (which keeps running in the background once opened),
//! the screencast is started only while the panel is visible and stopped
//! the moment it's hidden - CPU discipline the design doc calls out
//! explicitly, since `Page.startScreencast` keeps Chrome compositing and
//! encoding frames for as long as it's active.

use std::sync::Arc;
use std::time::Duration;

use jian_widgets::Rect;
use zode_app_model::SecondaryPane;
use zode_core::browser::{BrowserSession, BrowserTarget};
use zode_node_protocol::NodeCapability;

use super::DesktopApp;
use crate::window_state::AppWake;

/// How often the background ticker wakes the event loop while the panel is
/// visible. Screencast frames don't arrive through any existing wake path
/// (unlike terminal output, which has its own reader-thread callback), so
/// this is what actually gets `poll_browser_frame` called on a live
/// cadence. ~12.5 Hz comfortably exceeds the panel's own JPEG quality/size
/// budget without adding a second high-frequency timer to the app.
const BROWSER_WAKE_INTERVAL: Duration = Duration::from_millis(80);

/// A decoded-ready frame cached on the desktop app shell between ticks.
/// `image_id` is fresh per *new* frame (see `DesktopApp::poll_browser_frame`)
/// and reused across repeat paints of the same frame - the panel widget's
/// underlying image cache is keyed by identity, not content.
pub(super) struct BrowserFrameCache {
    pub image_id: u64,
    pub bytes: Arc<Vec<u8>>,
}

/// What changed after a poll: `state_changed` (accessibility-visible;
/// e.g. `has_frame` flipped) warrants a full snapshot rebuild, while
/// `new_frame` (a fresh bitmap, no state change) only needs a redraw - the
/// frame bytes live outside `ZodeAppState` on purpose, see
/// `BrowserPanelState`'s doc comment.
pub(super) struct BrowserFramePoll {
    pub state_changed: bool,
    pub new_frame: bool,
}

impl DesktopApp {
    /// Starts the frame stream for the current panel size. Called once on
    /// the open transition (see `apply_presentation_command`), not on every
    /// tick - `poll_browser_frame` is the hot path.
    pub(super) fn ensure_browser_runtime(&mut self) {
        let Some(session) = self.browser_session.clone() else {
            self.app_state.browser.unavailable_reason = Some("浏览器未连接。".into());
            return;
        };
        if !self
            .app_state
            .host
            .capabilities
            .capabilities
            .contains(&NodeCapability::Browser)
        {
            self.app_state.browser.unavailable_reason = Some("当前节点不支持浏览器。".into());
            return;
        }
        self.app_state.browser.is_bridge_target = matches!(session.target(), BrowserTarget::Bridge);
        if !session.supports_frame_stream() {
            self.app_state.browser.unavailable_reason = Some("扩展连接模式暂不支持预览。".into());
            return;
        }
        self.app_state.browser.unavailable_reason = None;
        let max_width = self.browser_frame_max_width();
        spawn_frame_stream_start(session, max_width);
        self.start_browser_wake_task();
    }

    /// Stops the frame stream and clears the last-seen frame - the panel
    /// goes back to its idle placeholder rather than a stale image the next
    /// time it's opened.
    pub(super) fn stop_browser_runtime(&mut self) {
        if let Some(task) = self.browser_wake_task.take() {
            task.abort();
        }
        self.browser_frame = None;
        self.app_state.browser.has_frame = false;
        if let Some(session) = self.browser_session.clone() {
            tokio::spawn(async move {
                let _ = session.stop_frame_stream().await;
            });
        }
    }

    /// Re-requests the stream at a new `max_width` so a resized panel isn't
    /// stuck upscaling (blurry) or downscaling (wasted bandwidth) a frame
    /// sized for the old rect.
    pub(super) fn resize_browser_frame_stream(&mut self) {
        if self.app_state.presentation.secondary_pane != Some(SecondaryPane::Browser)
            || self.app_state.browser.unavailable_reason.is_some()
        {
            return;
        }
        let Some(session) = self.browser_session.clone() else {
            return;
        };
        spawn_frame_stream_start(session, self.browser_frame_max_width());
    }

    /// Drains whatever the screencast listener has produced since the last
    /// poll. Non-blocking (`latest_frame_hint` never waits behind an
    /// in-flight agent tool call) and a no-op unless the panel is actually
    /// visible - decode discipline: only decode when visible AND a new
    /// frame arrived, never on every UI tick.
    pub(super) fn poll_browser_frame(&mut self) -> BrowserFramePoll {
        let none = BrowserFramePoll {
            state_changed: false,
            new_frame: false,
        };
        if self.app_state.presentation.secondary_pane != Some(SecondaryPane::Browser) {
            return none;
        }
        let Some(session) = self.browser_session.as_ref() else {
            return none;
        };
        let Some(frame) = session.latest_frame_hint() else {
            return none;
        };
        if frame.sequence == self.browser_frame_seq {
            return none;
        }
        self.browser_frame_seq = frame.sequence;
        self.browser_image_id = self.browser_image_id.wrapping_add(1);
        self.browser_frame = Some(BrowserFrameCache {
            image_id: self.browser_image_id,
            bytes: frame.data,
        });
        let had_frame = self.app_state.browser.has_frame;
        self.app_state.browser.has_frame = true;
        BrowserFramePoll {
            state_changed: !had_frame,
            new_frame: true,
        }
    }

    /// Cheap `Arc` clone of the cached frame, decoupled from `&self` so the
    /// caller can hold it across the mutable borrows the paint path takes
    /// (raster surface, renderer, accessibility tree) before constructing
    /// the actual `BrowserFrameView` right at the paint call.
    pub(super) fn browser_frame_snapshot(&self) -> Option<(u64, Arc<Vec<u8>>)> {
        self.browser_frame
            .as_ref()
            .map(|frame| (frame.image_id, frame.bytes.clone()))
    }

    pub(super) fn browser_rect(&self) -> Rect {
        if self.app_state.presentation.secondary_pane != Some(SecondaryPane::Browser) {
            return Rect::xywh(0.0, 0.0, 0.0, 0.0);
        }
        let geometry = self.frame_snapshot.layout;
        let panel = if geometry.review_panel.size.x > 0.0 {
            geometry.review_panel
        } else {
            geometry.primary_surface
        };
        zode_app_ui::BrowserPanel::layout(panel).canvas
    }

    /// Physical (device) pixel width for `Page.startScreencast`'s
    /// `maxWidth` - capping there, not the logical panel width, is what
    /// keeps a HiDPI panel from upscaling a blurry stream.
    fn browser_frame_max_width(&self) -> u32 {
        let logical_width = self.browser_rect().size.x.max(1.0);
        let physical = logical_width * self.window_state.scale_factor as f32;
        physical.round().max(1.0) as u32
    }

    fn start_browser_wake_task(&mut self) {
        if self.browser_wake_task.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        self.browser_wake_task = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(BROWSER_WAKE_INTERVAL);
            loop {
                interval.tick().await;
                if proxy.send_event(AppWake::Redraw).is_err() {
                    return;
                }
            }
        }));
    }
}

fn spawn_frame_stream_start(session: Arc<BrowserSession>, max_width: u32) {
    tokio::spawn(async move {
        let _ = session.start_frame_stream(max_width).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_app_model::BrowserPanelState;
    use zode_node_protocol::{CapabilityManifest, NodeId};

    fn capability_manifest(has_browser: bool) -> CapabilityManifest {
        let node_id = NodeId::new();
        let mut capabilities = std::collections::BTreeSet::new();
        if has_browser {
            capabilities.insert(NodeCapability::Browser);
        }
        CapabilityManifest {
            node_id,
            capabilities,
        }
    }

    #[test]
    fn browser_panel_state_defaults_are_idle_not_unavailable() {
        // A fresh `BrowserPanelState` (as constructed by `demo_state`)
        // should not itself claim unavailability - that's `DesktopApp`'s
        // job via `ensure_browser_runtime`, not a hardcoded default.
        let state = BrowserPanelState::default();
        assert!(state.unavailable_reason.is_none());
        assert!(!state.has_frame);
    }

    #[test]
    fn capability_manifest_gate_matches_missing_and_present_browser() {
        let missing = capability_manifest(false);
        assert!(!missing.capabilities.contains(&NodeCapability::Browser));
        let present = capability_manifest(true);
        assert!(present.capabilities.contains(&NodeCapability::Browser));
    }
}
