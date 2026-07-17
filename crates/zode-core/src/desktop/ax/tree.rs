//! App / window enumeration and the accessibility snapshot walk. App discovery
//! uses the on-screen window list (owner pid + name); windows and the element
//! tree come from AX. Native pointers stay on the actor thread.

#![cfg(target_os = "macos")]

use accessibility_sys::{
    kAXChildrenAttribute, kAXRoleAttribute, kAXSizeAttribute, kAXTitleAttribute, kAXValueAttribute,
    kAXWindowsAttribute,
};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    copy_window_info, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
};

use super::element::{attr_elem_array, attr_size, attr_string, create_app, AxElem};

/// One running app with an on-screen window.
pub struct AppRecord {
    pub pid: i32,
    pub name: String,
}

/// Enumerate on-screen apps (deduped by pid) via the window server list.
pub fn list_apps() -> Vec<AppRecord> {
    let mut seen: Vec<AppRecord> = Vec::new();
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let Some(info) = copy_window_info(options, 0) else {
        return seen;
    };
    // `info` is a CFArray of CFDictionary describing each window.
    let pid_key = CFString::from_static_string("kCGWindowOwnerPID");
    let name_key = CFString::from_static_string("kCGWindowOwnerName");
    for i in 0..info.len() {
        let Some(item) = info.get(i) else { continue };
        let dict = unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(*item as _) };
        let Some(pid) = dict
            .find(&pid_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i32())
        else {
            continue;
        };
        if pid <= 0 || seen.iter().any(|a| a.pid == pid) {
            continue;
        }
        let name = dict
            .find(&name_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("pid {pid}"));
        seen.push(AppRecord { pid, name });
    }
    seen
}

/// One window title (index is the token the model uses).
pub fn list_windows(pid: i32) -> Vec<Option<String>> {
    let app = create_app(pid);
    let windows = unsafe { attr_elem_array(app.as_ref(), kAXWindowsAttribute) };
    windows
        .iter()
        .map(|w| unsafe { attr_string(w.as_ref(), kAXTitleAttribute) })
        .collect()
}

/// Fetch the AX element for the window at `index` of `pid`'s window list.
pub fn window_at(pid: i32, index: usize) -> Option<AxElem> {
    let app = create_app(pid);
    let mut windows = unsafe { attr_elem_array(app.as_ref(), kAXWindowsAttribute) };
    if index < windows.len() {
        Some(windows.swap_remove(index))
    } else {
        None
    }
}

/// Walk the window subtree into a ref-annotated outline and a parallel element
/// map (outline `[e<n>]` ↔ `nodes[n-1]`). Deterministic, depth-first, capped at
/// `max_nodes` (spec §快照与裁剪). Returns `(outline, nodes)`.
pub fn snapshot(root: &AxElem, max_nodes: usize) -> (String, Vec<AxElem>) {
    let mut nodes: Vec<AxElem> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    walk(
        root.clone(),
        0,
        max_nodes,
        &mut nodes,
        &mut lines,
        &mut truncated,
    );
    if truncated {
        lines.push("  …truncated, use snapshot with scope to expand".to_string());
    }
    (lines.join("\n"), nodes)
}

fn walk(
    elem: AxElem,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<AxElem>,
    lines: &mut Vec<String>,
    truncated: &mut bool,
) {
    if nodes.len() >= max_nodes {
        *truncated = true;
        return;
    }
    let role = unsafe { attr_string(elem.as_ref(), kAXRoleAttribute) }
        .unwrap_or_else(|| "Unknown".to_string());
    let label = unsafe { attr_string(elem.as_ref(), kAXTitleAttribute) }
        .or_else(|| unsafe { attr_string(elem.as_ref(), kAXValueAttribute) })
        .unwrap_or_default();
    let offscreen = unsafe { attr_size(elem.as_ref(), kAXSizeAttribute) }
        .map(|s| s.width <= 0.0 || s.height <= 0.0)
        .unwrap_or(false);

    nodes.push(elem.clone());
    let n = nodes.len();
    let indent = "  ".repeat(depth);
    let mut line = if label.is_empty() {
        format!("{indent}[e{n}] {role}")
    } else {
        format!("{indent}[e{n}] {role} {label:?}")
    };
    if offscreen {
        line.push_str(" (offscreen)");
    }
    lines.push(line);

    let children = unsafe { attr_elem_array(elem.as_ref(), kAXChildrenAttribute) };
    for child in children {
        if nodes.len() >= max_nodes {
            *truncated = true;
            break;
        }
        walk(child, depth + 1, max_nodes, nodes, lines, truncated);
    }
}
