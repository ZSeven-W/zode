//! CDP event plumbing for [`ManagedBackend`](super::managed::ManagedBackend):
//! the console/network log listeners, their event-to-entry conversions,
//! and the spectator screencast listener. Split out of `managed.rs` to
//! keep that file under the 800-line limit.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use base64::Engine as _;
use chromiumoxide::cdp::browser_protocol::network::{
    EventRequestWillBeSent, EventResponseReceived, RequestId,
};
use chromiumoxide::cdp::browser_protocol::page::{EventScreencastFrame, ScreencastFrameAckParams};
use chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;
use chromiumoxide::Page;
use futures::StreamExt;

use super::backend::{ConsoleEntry, NetworkEntry, ScreencastFrame};

/// Cap for the console/network ring buffers and the request-correlation
/// pending queue; oldest entries are evicted once exceeded.
pub(super) const LOG_BUFFER_CAP: usize = 500;

/// Spawns the screencast frame listener task for `page`: decodes each
/// base64 JPEG frame, stores it in the single-slot `frame` cell (overwrite,
/// never queue — a slow consumer sees only the newest frame), bumps
/// `sequence`, and acks the frame so Chrome keeps streaming (CDP pauses
/// screencast delivery until each frame is acknowledged). A frame that
/// fails to base64-decode is skipped but still acked, so one malformed
/// frame can't stall the stream. Returns the task handle so the caller can
/// abort it on stop / tab swap / restart.
pub(super) fn attach_screencast_listener(
    page: Page,
    frame: Arc<StdMutex<Option<ScreencastFrame>>>,
    sequence: Arc<AtomicU64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(mut events) = page.event_listener::<EventScreencastFrame>().await else {
            return;
        };
        while let Some(event) = events.next().await {
            let raw: &[u8] = event.data.as_ref();
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
                let next = sequence.fetch_add(1, Ordering::SeqCst) + 1;
                *frame.lock().unwrap() = Some(ScreencastFrame {
                    data: Arc::new(bytes),
                    sequence: next,
                });
            }
            let ack = ScreencastFrameAckParams::new(event.session_id);
            if page.execute(ack).await.is_err() {
                break;
            }
        }
    })
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
/// join handles so the caller
/// ([`replace_listeners`](super::managed::ManagedBackend)) can abort them
/// once `page` is no longer current. chromiumoxide scopes event listeners
/// to a single `Page`, so this must be re-run every time the backend's
/// current page is swapped (see `tab_new`/`tab_select`) or the new tab's
/// activity won't be captured — and the PREVIOUS page's tasks must be
/// aborted or they keep running (and writing into the shared buffers)
/// indefinitely, since the old `Page` handle isn't dropped, only replaced.
pub(super) fn attach_listeners(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_evicts_the_oldest_entry() {
        let buf: StdMutex<VecDeque<usize>> = StdMutex::new(VecDeque::new());
        for i in 0..LOG_BUFFER_CAP + 5 {
            push_capped(&buf, i);
        }
        let guard = buf.lock().unwrap();
        assert_eq!(guard.len(), LOG_BUFFER_CAP);
        assert_eq!(*guard.front().unwrap(), 5);
        assert_eq!(*guard.back().unwrap(), LOG_BUFFER_CAP + 4);
    }
}
