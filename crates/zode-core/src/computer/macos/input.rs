//! Input injection via `CGEvent`: mouse click/drag, keyboard (named keys +
//! arbitrary Unicode text), and scroll. All events post at the HID system
//! event tap (`CGEventTapLocation::HIDEventTap`), i.e. as if a real user
//! acted — there is no per-app targeting at this layer; `AXUIElementPerformAction`
//! (see `ax.rs`) is preferred for clicks when an element ref resolves, since
//! it does not depend on window focus/z-order the way synthetic mouse
//! coordinates do.

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType, CGMouseButton,
    CGScrollEventUnit,
};

use super::super::backend::ComputerError;

fn protocol_err(what: &str) -> ComputerError {
    ComputerError::Protocol(format!("failed to create {what} CGEvent"))
}

fn event_source() -> Option<objc2_core_foundation::CFRetained<CGEventSource>> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
}

fn post_mouse(down: CGEventType, up: CGEventType, point: CGPoint) -> Result<(), ComputerError> {
    let source = event_source();
    let down_event = CGEvent::new_mouse_event(source.as_deref(), down, point, CGMouseButton::Left)
        .ok_or_else(|| protocol_err("mouse-down"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down_event));
    let up_event = CGEvent::new_mouse_event(source.as_deref(), up, point, CGMouseButton::Left)
        .ok_or_else(|| protocol_err("mouse-up"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up_event));
    Ok(())
}

/// Click (mouse down + up) at global logical coordinates.
pub(super) fn click_at(x: f64, y: f64) -> Result<(), ComputerError> {
    post_mouse(
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGPoint::new(x, y),
    )
}

/// Drag from `(from_x, from_y)` to `(to_x, to_y)`: mouse down at the start,
/// a dragged event at the end point, then mouse up there.
pub(super) fn drag(from: (f64, f64), to: (f64, f64)) -> Result<(), ComputerError> {
    let source = event_source();
    let from_point = CGPoint::new(from.0, from.1);
    let to_point = CGPoint::new(to.0, to.1);

    let down = CGEvent::new_mouse_event(
        source.as_deref(),
        CGEventType::LeftMouseDown,
        from_point,
        CGMouseButton::Left,
    )
    .ok_or_else(|| protocol_err("mouse-down"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));

    let dragged = CGEvent::new_mouse_event(
        source.as_deref(),
        CGEventType::LeftMouseDragged,
        to_point,
        CGMouseButton::Left,
    )
    .ok_or_else(|| protocol_err("mouse-dragged"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&dragged));

    let up = CGEvent::new_mouse_event(
        source.as_deref(),
        CGEventType::LeftMouseUp,
        to_point,
        CGMouseButton::Left,
    )
    .ok_or_else(|| protocol_err("mouse-up"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

/// Type arbitrary Unicode text via a single keyDown/keyUp pair carrying the
/// whole string as its Unicode payload (`CGEventKeyboardSetUnicodeString`) —
/// the standard technique for injecting text that isn't a single keystroke,
/// since it sidesteps per-character virtual-keycode/layout mapping entirely.
pub(super) fn type_text(text: &str) -> Result<(), ComputerError> {
    if text.is_empty() {
        return Ok(());
    }
    let source = event_source();
    let units: Vec<u16> = text.encode_utf16().collect();
    let down = CGEvent::new_keyboard_event(source.as_deref(), 0, true)
        .ok_or_else(|| protocol_err("keyDown"))?;
    unsafe {
        CGEvent::keyboard_set_unicode_string(Some(&down), units.len() as u64, units.as_ptr());
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));

    let up = CGEvent::new_keyboard_event(source.as_deref(), 0, false)
        .ok_or_else(|| protocol_err("keyUp"))?;
    unsafe {
        CGEvent::keyboard_set_unicode_string(Some(&up), units.len() as u64, units.as_ptr());
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

/// Map a named key (case-insensitive) to its macOS virtual keycode.
fn keycode_for(name: &str) -> Option<u16> {
    Some(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => 0x24,
        "tab" => 0x30,
        "space" => 0x31,
        "backspace" => 0x33,
        "escape" | "esc" => 0x35,
        "delete" | "forwarddelete" => 0x75,
        "home" => 0x73,
        "end" => 0x77,
        "pageup" => 0x74,
        "pagedown" => 0x79,
        "arrowleft" | "left" => 0x7B,
        "arrowright" | "right" => 0x7C,
        "arrowdown" | "down" => 0x7D,
        "arrowup" | "up" => 0x7E,
        _ => return None,
    })
}

/// Press and release a named key (e.g. "Enter", "Escape", "ArrowDown").
pub(super) fn key_press(name: &str) -> Result<(), ComputerError> {
    let code = keycode_for(name)
        .ok_or_else(|| ComputerError::NotFound(format!("unknown key name: {name:?}")))?;
    let source = event_source();
    let down = CGEvent::new_keyboard_event(source.as_deref(), code, true)
        .ok_or_else(|| protocol_err("keyDown"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&down));
    let up = CGEvent::new_keyboard_event(source.as_deref(), code, false)
        .ok_or_else(|| protocol_err("keyUp"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&up));
    Ok(())
}

/// Scroll by `dx`, `dy` pixels at the current pointer location.
pub(super) fn scroll(dx: f64, dy: f64) -> Result<(), ComputerError> {
    let source = event_source();
    let event = CGEvent::new_scroll_wheel_event2(
        source.as_deref(),
        CGScrollEventUnit::Pixel,
        2,
        dy as i32,
        dx as i32,
        0,
    )
    .ok_or_else(|| protocol_err("scroll"))?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_keys_map_to_stable_codes() {
        assert_eq!(keycode_for("Enter"), Some(0x24));
        assert_eq!(keycode_for("ESCAPE"), Some(0x35));
        assert_eq!(keycode_for("ArrowDown"), Some(0x7D));
        assert_eq!(keycode_for("nonexistent-key"), None);
    }
}
