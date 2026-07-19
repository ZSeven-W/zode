//! Element actions, window focus, keyboard synthesis, pointer synthesis, and
//! window screenshot for the macOS AX backend. Semantic AX actions are tried
//! first; when an element exposes none (self-drawn UI), M4 pointer synthesis
//! clicks/scrolls at the element's center via CGEvent. Key combos are atomic
//! down/up with guaranteed key-up cleanup on every exit path (spec §输入安全).

#![cfg(target_os = "macos")]

use accessibility_sys::{
    kAXErrorSuccess, kAXMainAttribute, kAXPositionAttribute, kAXPressAction, kAXRaiseAction,
    kAXSizeAttribute, kAXValueAttribute, AXUIElementCopyActionNames, AXUIElementPerformAction,
    AXUIElementSetAttributeValue,
};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use crate::desktop::backend::{DesktopError, ElementActionKind};

use super::element::{attr_point, attr_size, AxElem};

/// Perform an action on an element. Semantic AX action first (AXPress); when the
/// element exposes no matching action, fall back to M4 pointer synthesis at the
/// element's center. Scroll is always a synthesized wheel event at the center.
pub fn element_action(elem: &AxElem, kind: ElementActionKind) -> Result<String, DesktopError> {
    match kind {
        ElementActionKind::Click | ElementActionKind::Toggle | ElementActionKind::Expand => {
            if supports_action(elem, kAXPressAction) {
                let cf = CFString::new(kAXPressAction);
                let err =
                    unsafe { AXUIElementPerformAction(elem.as_ref(), cf.as_concrete_TypeRef()) };
                return if err == kAXErrorSuccess {
                    Ok("ok:press".into())
                } else {
                    Err(DesktopError::Protocol(format!(
                        "AXPerformAction failed: {err}"
                    )))
                };
            }
            // Pointer-synthesis fallback (M4): click the element center.
            let (x, y) = element_center(elem)?;
            mouse_click(x, y)?;
            Ok("ok:click".into())
        }
        ElementActionKind::Scroll => {
            let (x, y) = element_center(elem)?;
            wheel_scroll(x, y, -3)?;
            Ok("ok:scroll".into())
        }
    }
}

/// Compute an element's center point in main-display screen coordinates
/// (top-left origin), from AXPosition + AXSize.
pub(super) fn element_center(elem: &AxElem) -> Result<(f64, f64), DesktopError> {
    let p = unsafe { attr_point(elem.as_ref(), kAXPositionAttribute) }
        .ok_or_else(|| DesktopError::NotFound("element has no AXPosition".into()))?;
    let s = unsafe { attr_size(elem.as_ref(), kAXSizeAttribute) }
        .ok_or_else(|| DesktopError::NotFound("element has no AXSize".into()))?;
    Ok((p.x + s.width / 2.0, p.y + s.height / 2.0))
}

/// Owning window's CGWindowID for an element (AXWindow → frame → CGWindowList
/// match). Best-effort ghost-cursor targeting: any failure returns None.
pub(super) fn cg_window_id_for_element(pid: i32, elem: &AxElem) -> Option<u32> {
    use super::element::copy_attr;
    use accessibility_sys::{AXUIElementGetTypeID, AXUIElementRef};
    use core_foundation::base::{CFGetTypeID, CFRelease};

    let raw = unsafe { copy_attr(elem.as_ref(), "AXWindow") }?;
    if unsafe { CFGetTypeID(raw) } != unsafe { AXUIElementGetTypeID() } {
        unsafe { CFRelease(raw) };
        return None;
    }
    let win = unsafe { AxElem::from_create(raw as AXUIElementRef) };
    let p = unsafe { attr_point(win.as_ref(), kAXPositionAttribute) }?;
    let s = unsafe { attr_size(win.as_ref(), kAXSizeAttribute) }?;
    resolve_cg_window_id(pid, p.x, p.y, s.width, s.height).ok()
}

/// Synthesize a left click at a screen point (down then up).
fn mouse_click(x: f64, y: f64) -> Result<(), DesktopError> {
    let src = event_source()?;
    let pt = CGPoint::new(x, y);
    for ty in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
        let ev = CGEvent::new_mouse_event(src.clone(), ty, pt, CGMouseButton::Left)
            .map_err(|_| DesktopError::Protocol("mouse event create failed".into()))?;
        ev.post(CGEventTapLocation::HID);
    }
    Ok(())
}

/// Synthesize a vertical wheel scroll at a screen point (`lines` < 0 scrolls
/// content down). Moves the cursor there first so the event targets the point.
fn wheel_scroll(x: f64, y: f64, lines: i32) -> Result<(), DesktopError> {
    let src = event_source()?;
    let mv = CGEvent::new_mouse_event(
        src.clone(),
        CGEventType::MouseMoved,
        CGPoint::new(x, y),
        CGMouseButton::Left,
    )
    .map_err(|_| DesktopError::Protocol("mouse move create failed".into()))?;
    mv.post(CGEventTapLocation::HID);
    let ev = CGEvent::new_scroll_event(src, ScrollEventUnit::LINE, 1, lines, 0, 0)
        .map_err(|_| DesktopError::Protocol("scroll event create failed".into()))?;
    ev.post(CGEventTapLocation::HID);
    Ok(())
}

fn supports_action(elem: &AxElem, action: &str) -> bool {
    let mut names: CFArrayRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyActionNames(elem.as_ref(), &mut names) };
    if err != kAXErrorSuccess || names.is_null() {
        return false;
    }
    let arr: CFArray<CFString> = unsafe { CFArray::wrap_under_create_rule(names) };
    arr.iter().any(|n| n.to_string() == action)
}

/// Set an element's `AXValue` string. Callers must have already rejected secure
/// fields (handled in the actor before dispatch).
pub fn set_value(elem: &AxElem, text: &str) -> Result<(), DesktopError> {
    let cf_attr = CFString::new(kAXValueAttribute);
    let cf_val = CFString::new(text);
    let err = unsafe {
        AXUIElementSetAttributeValue(
            elem.as_ref(),
            cf_attr.as_concrete_TypeRef(),
            cf_val.as_concrete_TypeRef() as CFTypeRef,
        )
    };
    if err == kAXErrorSuccess {
        Ok(())
    } else {
        Err(DesktopError::Protocol(format!(
            "AXSetAttributeValue failed: {err}"
        )))
    }
}

/// Raise a window and make it main.
pub fn focus_window(win: &AxElem) -> Result<(), DesktopError> {
    let cf_main = CFString::new(kAXMainAttribute);
    let t = CFBoolean::true_value();
    unsafe {
        AXUIElementSetAttributeValue(
            win.as_ref(),
            cf_main.as_concrete_TypeRef(),
            t.as_CFTypeRef(),
        );
    }
    let cf_raise = CFString::new(kAXRaiseAction);
    let err = unsafe { AXUIElementPerformAction(win.as_ref(), cf_raise.as_concrete_TypeRef()) };
    if err == kAXErrorSuccess {
        Ok(())
    } else {
        // Making it main may still have worked; report raise failure softly.
        Err(DesktopError::Protocol(format!("AXRaise failed: {err}")))
    }
}

fn event_source() -> Result<CGEventSource, DesktopError> {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| DesktopError::Protocol("CGEventSource create failed".into()))
}

/// True when `text` contains a char with no virtual keycode: such text must
/// go through the pasteboard path, because apps with custom key handling read
/// the keycode, not the unicode payload (keycode 0 is kVK_ANSI_A — they would
/// see every payload-only char as "a").
pub fn needs_paste(text: &str) -> bool {
    !text.chars().all(|c| char_keycode(c).is_some())
}

/// Type text one character at a time. `sent` tracks progress for `PartialInput`
/// reporting if a later refactor adds mid-stream focus checks (M1 posts all).
///
/// Each event carries the character as its unicode payload plus its real
/// US-layout keycode. Text that `needs_paste` is delivered via the pasteboard
/// instead (see `paste.rs`).
pub fn type_text(pid: i32, text: &str) -> Result<(), DesktopError> {
    if needs_paste(text) {
        return super::paste::paste_text(pid, text);
    }
    let src = event_source()?;
    for (sent, ch) in text.chars().enumerate() {
        let mut buf = [0u16; 2];
        let utf16 = ch.encode_utf16(&mut buf);
        let code = char_keycode(ch).unwrap_or(0);
        let down = CGEvent::new_keyboard_event(src.clone(), code, true).map_err(|_| {
            DesktopError::PartialInput {
                characters_sent: sent,
                reason: "keydown create failed".into(),
            }
        })?;
        down.set_string_from_utf16_unchecked(utf16);
        down.post_to_pid(pid);
        let up = CGEvent::new_keyboard_event(src.clone(), code, false).map_err(|_| {
            DesktopError::PartialInput {
                characters_sent: sent,
                reason: "keyup create failed".into(),
            }
        })?;
        up.set_string_from_utf16_unchecked(utf16);
        up.post_to_pid(pid);
    }
    Ok(())
}

/// Press one key combo atomically: modifiers down, key down/up, modifiers up.
/// Every exit path releases what it pressed — no stuck modifiers (spec §输入安全).
pub fn key_combo(pid: i32, combo: &str) -> Result<(), DesktopError> {
    let (mods, key) = parse_combo(combo)?;
    let src = event_source()?;
    let mut flags = CGEventFlags::empty();
    for m in &mods {
        flags |= m.flag();
    }
    // Key down with modifier flags set, then key up. CGEvent modifier flags
    // model the combo atomically, so there is no separate modifier key to leak.
    let post = |keydown: bool| -> Result<(), DesktopError> {
        let ev = CGEvent::new_keyboard_event(src.clone(), key, keydown)
            .map_err(|_| DesktopError::Protocol("keyboard event create failed".into()))?;
        ev.set_flags(flags);
        ev.post_to_pid(pid);
        Ok(())
    };
    post(true)?;
    // Always attempt the key-up even if down failed mid-way (belt and braces).
    let up = post(false);
    up.map_err(|_| DesktopError::PartialKeyInput {
        combos_sent: 0,
        cleanup_ok: false,
        reason: "key-up failed; modifier state may be inconsistent".into(),
    })
}

struct Modifier(CGEventFlags);
impl Modifier {
    fn flag(&self) -> CGEventFlags {
        self.0
    }
}

fn parse_combo(combo: &str) -> Result<(Vec<Modifier>, CGKeyCode), DesktopError> {
    let mut mods = Vec::new();
    let mut key: Option<CGKeyCode> = None;
    for part in combo.split('+').map(|p| p.trim()) {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" | "super" => {
                mods.push(Modifier(CGEventFlags::CGEventFlagCommand))
            }
            "shift" => mods.push(Modifier(CGEventFlags::CGEventFlagShift)),
            "alt" | "option" | "opt" => mods.push(Modifier(CGEventFlags::CGEventFlagAlternate)),
            "ctrl" | "control" => mods.push(Modifier(CGEventFlags::CGEventFlagControl)),
            other => {
                key = Some(keycode_for(other).ok_or_else(|| {
                    DesktopError::UnsupportedAction(format!(
                        "unknown key {other:?} in combo {combo:?}"
                    ))
                })?);
            }
        }
    }
    key.map(|k| (mods, k))
        .ok_or_else(|| DesktopError::UnsupportedAction(format!("no key in combo {combo:?}")))
}

fn keycode_for(name: &str) -> Option<CGKeyCode> {
    use core_graphics::event::KeyCode;
    let single = name.chars().next()?;
    if name.len() == 1 {
        if let Some(k) = letter_keycode(single) {
            return Some(k);
        }
    }
    Some(match name {
        "enter" | "return" => KeyCode::RETURN,
        "tab" => KeyCode::TAB,
        "space" => KeyCode::SPACE,
        "esc" | "escape" => KeyCode::ESCAPE,
        "delete" | "backspace" => KeyCode::DELETE,
        "up" => KeyCode::UP_ARROW,
        "down" => KeyCode::DOWN_ARROW,
        "left" => KeyCode::LEFT_ARROW,
        "right" => KeyCode::RIGHT_ARROW,
        "home" => KeyCode::HOME,
        "end" => KeyCode::END,
        _ => return None,
    })
}

/// Keycode for a literal typed character: letters/digits plus the whitespace
/// keys that occur inside typed text. Uppercase maps to the unshifted keycode;
/// the unicode payload still carries the exact character.
fn char_keycode(c: char) -> Option<CGKeyCode> {
    use core_graphics::event::KeyCode;
    match c {
        ' ' => Some(KeyCode::SPACE),
        '\n' | '\r' => Some(KeyCode::RETURN),
        '\t' => Some(KeyCode::TAB),
        _ => letter_keycode(c),
    }
}

/// US-layout virtual keycodes for letters and digits.
fn letter_keycode(c: char) -> Option<CGKeyCode> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a' => 0x00,
        'b' => 0x0B,
        'c' => 0x08,
        'd' => 0x02,
        'e' => 0x0E,
        'f' => 0x03,
        'g' => 0x05,
        'h' => 0x04,
        'i' => 0x22,
        'j' => 0x26,
        'k' => 0x28,
        'l' => 0x25,
        'm' => 0x2E,
        'n' => 0x2D,
        'o' => 0x1F,
        'p' => 0x23,
        'q' => 0x0C,
        'r' => 0x0F,
        's' => 0x01,
        't' => 0x11,
        'u' => 0x20,
        'v' => 0x09,
        'w' => 0x0D,
        'x' => 0x07,
        'y' => 0x10,
        'z' => 0x06,
        '0' => 0x1D,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '5' => 0x17,
        '6' => 0x16,
        '7' => 0x1A,
        '8' => 0x1C,
        '9' => 0x19,
        _ => return None,
    })
}

/// Capture a window as PNG bytes. Maps the AX window to a CGWindowID by owner
/// pid + bounds (fail-closed on weak/multiple matches), captures, and encodes.
pub fn screenshot(pid: i32, win: &AxElem) -> Result<Vec<u8>, DesktopError> {
    let pos = unsafe { attr_point(win.as_ref(), kAXPositionAttribute) }
        .ok_or_else(|| DesktopError::NotFound("window has no AXPosition".into()))?;
    let size = unsafe { attr_size(win.as_ref(), kAXSizeAttribute) }
        .ok_or_else(|| DesktopError::NotFound("window has no AXSize".into()))?;
    let window_id = resolve_cg_window_id(pid, pos.x, pos.y, size.width, size.height)?;
    super::screenshot::capture_window_png(window_id)
}

/// Find the single on-screen CGWindowID owned by `pid` whose bounds match the
/// AX rect within a small tolerance. Returns `Ambiguous`/`NotFound` otherwise.
fn resolve_cg_window_id(pid: i32, x: f64, y: f64, w: f64, h: f64) -> Result<u32, DesktopError> {
    use core_graphics::window::{
        copy_window_info, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let info = copy_window_info(options, 0)
        .ok_or_else(|| DesktopError::NotFound("window list unavailable".into()))?;
    let pid_key = CFString::from_static_string("kCGWindowOwnerPID");
    let num_key = CFString::from_static_string("kCGWindowNumber");
    let bounds_key = CFString::from_static_string("kCGWindowBounds");
    let (xk, yk, wk, hk) = (
        CFString::from_static_string("X"),
        CFString::from_static_string("Y"),
        CFString::from_static_string("Width"),
        CFString::from_static_string("Height"),
    );
    let mut matches: Vec<u32> = Vec::new();
    for i in 0..info.len() {
        let Some(item) = info.get(i) else { continue };
        let dict = unsafe {
            core_foundation::dictionary::CFDictionary::<
                CFString,
                core_foundation::base::CFType,
            >::wrap_under_get_rule(*item as _)
        };
        let owner = dict
            .find(&pid_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i32());
        if owner != Some(pid) {
            continue;
        }
        let Some(bounds_ref) = dict.find(&bounds_key) else {
            continue;
        };
        let bounds = unsafe {
            core_foundation::dictionary::CFDictionary::<CFString, core_foundation::base::CFType>::wrap_under_get_rule(
                bounds_ref.as_CFTypeRef() as core_foundation::dictionary::CFDictionaryRef,
            )
        };
        let get = |k: &CFString| -> Option<f64> {
            bounds
                .find(k)
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_f64())
        };
        let (bx, by, bw, bh) = match (get(&xk), get(&yk), get(&wk), get(&hk)) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => continue,
        };
        let close = (bx - x).abs() < 4.0
            && (by - y).abs() < 4.0
            && (bw - w).abs() < 4.0
            && (bh - h).abs() < 4.0;
        if close {
            if let Some(id) = dict
                .find(&num_key)
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_i64())
            {
                matches.push(id as u32);
            }
        }
    }
    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(DesktopError::NotFound(
            "no CGWindow matched the AX window bounds (fail closed)".into(),
        )),
        _ => Err(DesktopError::Ambiguous {
            candidates: matches.iter().map(|id| format!("cgwindow#{id}")).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_paste_classifies_by_keycode_coverage() {
        // Fully keycode-mappable: letters, digits, space, newline, tab.
        assert!(!needs_paste("hello world 123\n\tok"));
        // CJK has no virtual keycode.
        assert!(needs_paste("你好"));
        assert!(needs_paste("hi 许嘉天"));
        // ASCII punctuation is not in the keycode map either.
        assert!(needs_paste("hi!"));
        // Empty text needs no paste.
        assert!(!needs_paste(""));
    }
}
