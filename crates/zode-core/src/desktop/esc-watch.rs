//! Global Esc during desktop automation. A CGEventTap (macOS) on its own
//! CFRunLoop thread swallows Esc-keydown ONLY while armed and only when the
//! TUI receiver actually accepted the signal; everything else passes through
//! unmodified. Tap creation is lazy (first arm) and failure is non-fatal:
//! automation continues, Esc support is simply absent (debug log).
//!
//! Off-macOS every entry point is a no-op.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// macOS virtual keycode for Esc.
#[cfg(target_os = "macos")]
const KEYCODE_ESC: i64 = 53;

struct Watch {
    armed: AtomicBool,
    #[cfg(target_os = "macos")]
    tap_started: AtomicBool,
    tx: tokio::sync::mpsc::UnboundedSender<()>,
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>,
}

static WATCH: OnceLock<Watch> = OnceLock::new();

fn watch() -> &'static Watch {
    WATCH.get_or_init(|| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Watch {
            armed: AtomicBool::new(false),
            #[cfg(target_os = "macos")]
            tap_started: AtomicBool::new(false),
            tx,
            rx: Mutex::new(Some(rx)),
        }
    })
}

/// The TUI claims the fire channel exactly once at startup.
pub fn take_receiver() -> Option<tokio::sync::mpsc::UnboundedReceiver<()>> {
    watch().rx.lock().unwrap().take()
}

/// Arm while desktop automation is active. Idempotent; lazily starts the tap.
pub fn arm() {
    let w = watch();
    w.armed.store(true, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    start_tap_once(w);
    #[cfg(not(target_os = "macos"))]
    let _keep_receiver_open = &w.tx;
}

/// Disarm at turn end / after a fire. Esc passes through again immediately.
pub fn disarm() {
    watch().armed.store(false, Ordering::SeqCst);
}

#[cfg(test)]
pub fn fire_for_test() {
    let _ = watch().tx.send(());
}

#[cfg(target_os = "macos")]
fn start_tap_once(w: &'static Watch) {
    if w.tap_started.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("zode-esc-watch".into())
        .spawn(move || tap_thread(w));
    if spawned.is_err() {
        tracing::debug!("esc-watch: failed to spawn tap thread");
    }
}

#[cfg(target_os = "macos")]
fn tap_thread(w: &'static Watch) {
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };

    // Swallow (return None) ONLY when: KeyDown && armed && keycode==Esc && the
    // send was accepted (receiver alive). Every other event is returned
    // unmodified so it continues to the frontmost app. In core-graphics 0.24
    // returning `None` deletes the event; `Some(event)` passes it on.
    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown],
        |_proxy, _etype, event| {
            let is_esc =
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) == KEYCODE_ESC;
            if is_esc && w.armed.load(Ordering::SeqCst) && w.tx.send(()).is_ok() {
                None // swallow: the Esc became a turn interrupt
            } else {
                Some(event.clone())
            }
        },
    );
    let Ok(tap) = tap else {
        tracing::debug!("esc-watch: CGEventTap creation failed (AX trust missing?)");
        return;
    };
    let Ok(source) = tap.mach_port.create_runloop_source(0) else {
        tracing::debug!("esc-watch: runloop source creation failed");
        return;
    };
    let rl = CFRunLoop::get_current();
    unsafe {
        rl.add_source(&source, kCFRunLoopCommonModes);
    }
    tap.enable();
    CFRunLoop::run_current();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single test on purpose: the watch is process-global state; splitting
    /// into several #[test] fns would race over take_receiver().
    #[tokio::test]
    async fn receiver_take_once_arm_disarm_and_fire() {
        let mut rx = take_receiver().expect("first take yields the receiver");
        assert!(take_receiver().is_none(), "second take is None");

        // arm/disarm never panic on any platform (in CI the tap thread just
        // logs and exits when AX trust is missing).
        arm();
        disarm();

        fire_for_test();
        assert_eq!(rx.recv().await, Some(()));
    }
}
