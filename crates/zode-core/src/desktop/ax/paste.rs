//! Clipboard-paste text path for text that keyboard synthesis cannot type:
//! any char without a virtual keycode (CJK, punctuation) is delivered by
//! writing the pasteboard and synthesizing Cmd+V, because apps with custom key
//! handling read the virtual keycode and would render every payload-only char
//! as "a" (keycode 0 = kVK_ANSI_A).
//!
//! The previous pasteboard text is restored best-effort after the paste; a
//! non-text pasteboard (image, files) cannot be saved and is lost — documented
//! M1 limitation.

#![cfg(target_os = "macos")]
// The cocoa crate deprecated its whole surface in favor of objc2; the
// workspace stays on cocoa (zode-overlay does the same).
#![allow(deprecated)]

use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSAutoreleasePool, NSString};

use crate::desktop::backend::DesktopError;

/// How long the target gets to read the pasteboard after Cmd+V before the
/// previous contents are restored. Paste handlers read synchronously on the
/// key event, so this only needs to cover event-queue delivery.
const PASTE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// A Cocoa pasteboard handle. Production uses the general pasteboard; tests
/// use a uniquely named one so they never touch the user's clipboard.
pub struct Pasteboard(id);

impl Pasteboard {
    pub fn general() -> Self {
        Pasteboard(unsafe { NSPasteboard::generalPasteboard(nil) })
    }

    /// A named app-local pasteboard (test isolation).
    #[cfg(test)]
    pub fn with_name(name: &str) -> Self {
        Pasteboard(unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let ns = NSString::alloc(nil).init_str(name).autorelease();
            let pb = NSPasteboard::pasteboardWithName(nil, ns);
            pool.drain();
            pb
        })
    }

    /// Current plain-text contents, if the pasteboard holds text.
    pub fn string(&self) -> Option<String> {
        unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let s: id = self.0.stringForType(NSPasteboardTypeString);
            let out = if s == nil {
                None
            } else {
                let c = NSString::UTF8String(s);
                if c.is_null() {
                    None
                } else {
                    Some(std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned())
                }
            };
            pool.drain();
            out
        }
    }

    pub fn set_string(&self, text: &str) -> Result<(), DesktopError> {
        unsafe {
            let pool = NSAutoreleasePool::new(nil);
            self.0.clearContents();
            let ns = NSString::alloc(nil).init_str(text).autorelease();
            let ok = self.0.setString_forType(ns, NSPasteboardTypeString);
            pool.drain();
            if ok == cocoa::base::YES {
                Ok(())
            } else {
                Err(DesktopError::Protocol("pasteboard write failed".into()))
            }
        }
    }
}

/// Deliver `text` to `pid` by pasteboard + Cmd+V, restoring the previous
/// pasteboard text afterwards (best-effort, even when the combo fails).
pub fn paste_text(pid: i32, text: &str) -> Result<(), DesktopError> {
    let pb = Pasteboard::general();
    let saved = pb.string();
    pb.set_string(text)?;
    let combo = super::input::key_combo(pid, "cmd+v");
    std::thread::sleep(PASTE_SETTLE);
    if let Some(old) = saved {
        let _ = pb.set_string(&old);
    }
    combo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_pasteboard_round_trips_text() {
        let pb = Pasteboard::with_name("zode-test-paste-round-trip");
        pb.set_string("许嘉天 hello ✓").unwrap();
        assert_eq!(pb.string().as_deref(), Some("许嘉天 hello ✓"));
        pb.set_string("second write replaces").unwrap();
        assert_eq!(pb.string().as_deref(), Some("second write replaces"));
    }
}
