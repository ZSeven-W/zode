//! Accessibility (AX) tree read/write helpers: application enumeration,
//! bounded tree walk producing a ref-annotated outline (mirrors the
//! `[N] <tag> role text` shape of `browser/snapshot_js.rs`), path-based
//! element re-resolution for acting on a previously-read ref, and the two
//! write primitives (`AXPress` action, direct `AXValue` write).
//!
//! Elements are cached as a *path* (child-index list from the app root),
//! not a live `AXUIElement` handle: re-deriving the element from its path
//! at act time avoids holding a CF object across an `await` point and
//! keeps the cache trivially `Send`.

use std::ptr::NonNull;

use libc::pid_t;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGPoint, CGSize};

use super::super::backend::ComputerError;

const MAX_DEPTH: usize = 12;
const MAX_NODES: usize = 400;

/// A cached outline entry: enough to re-find the element later (by path)
/// and to describe it in an approval prompt without re-walking the tree.
#[derive(Debug, Clone)]
pub(super) struct CachedElement {
    pub path: Vec<i64>,
    pub role: String,
    pub label: String,
    pub point: (f64, f64),
}

pub(super) struct WalkResult {
    pub outline: String,
    pub elements: Vec<CachedElement>,
}

fn to_agent_ax_err(context: &str, err: AXError) -> ComputerError {
    ComputerError::Protocol(format!("{context} failed: AXError({})", err.0))
}

/// Copy a CF attribute value off `element`, or `None` if unsupported/absent.
/// Uses the CF "Copy" ownership rule: the returned pointer is already +1
/// owned, so we wrap it with `from_raw` (no extra retain).
fn copy_attribute(element: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let attr = CFString::from_str(name);
    let mut out: *const CFType = std::ptr::null();
    let err = unsafe { element.copy_attribute_value(&attr, NonNull::from(&mut out)) };
    if err != AXError::Success || out.is_null() {
        return None;
    }
    // SAFETY: `out` is a non-null +1 CF reference per the Copy rule above.
    Some(unsafe { CFRetained::from_raw(NonNull::new(out as *mut CFType).unwrap()) })
}

fn attr_string(element: &AXUIElement, name: &str) -> Option<String> {
    let value = copy_attribute(element, name)?;
    let s = value.downcast::<CFString>().ok()?;
    Some(s.to_string())
}

fn attr_point(element: &AXUIElement, name: &str) -> Option<(f64, f64)> {
    let value = copy_attribute(element, name)?;
    let axv = value.downcast::<AXValue>().ok()?;
    let mut point = CGPoint::ZERO;
    let ok = unsafe {
        axv.value(
            AXValueType::CGPoint,
            NonNull::from(&mut point).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some((point.x, point.y))
}

fn attr_size(element: &AXUIElement, name: &str) -> Option<(f64, f64)> {
    let value = copy_attribute(element, name)?;
    let axv = value.downcast::<AXValue>().ok()?;
    let mut size = CGSize::ZERO;
    let ok = unsafe {
        axv.value(
            AXValueType::CGSize,
            NonNull::from(&mut size).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some((size.width, size.height))
}

/// Children of `element` via the `AXChildren` attribute. `CFArray`'s
/// default element type (`Opaque`) doesn't implement the `Type` bound the
/// safe `get`/`iter` wrappers require, so this walks the raw
/// `CFArrayGetValueAtIndex` API directly. That call follows the CF "Get"
/// rule — a borrowed reference — so (unlike `copy_attribute`) each item
/// must be retained before wrapping in `CFRetained`.
fn children_of(element: &AXUIElement) -> Vec<CFRetained<AXUIElement>> {
    let Some(value) = copy_attribute(element, "AXChildren") else {
        return Vec::new();
    };
    let Ok(array) = value.downcast::<CFArray>() else {
        return Vec::new();
    };
    let count = array.count();
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        // SAFETY: `i` is within `[0, count)`, so `value_at_index` returns a
        // valid (borrowed) CF object pointer or null.
        let ptr = unsafe { array.value_at_index(i) };
        if ptr.is_null() {
            continue;
        }
        let Some(nn) = NonNull::new(ptr as *mut CFType) else {
            continue;
        };
        // SAFETY: `nn` is a valid, borrowed CF object; `retain` bumps its
        // refcount so we own the `CFRetained` independent of the array.
        let retained: CFRetained<CFType> = unsafe { CFRetained::retain(nn) };
        if let Ok(el) = retained.downcast::<AXUIElement>() {
            out.push(el);
        }
    }
    out
}

fn role_and_label(element: &AXUIElement) -> (String, String) {
    let role = attr_string(element, "AXRole").unwrap_or_else(|| "AXUnknown".into());
    let label = attr_string(element, "AXTitle")
        .or_else(|| attr_string(element, "AXDescription"))
        .or_else(|| attr_string(element, "AXValue"))
        .unwrap_or_default();
    (role, label)
}

fn center_point(element: &AXUIElement) -> (f64, f64) {
    let pos = attr_point(element, "AXPosition").unwrap_or((0.0, 0.0));
    let size = attr_size(element, "AXSize").unwrap_or((0.0, 0.0));
    (pos.0 + size.0 / 2.0, pos.1 + size.1 / 2.0)
}

/// Depth-first walk of `element`'s subtree, bounded by `MAX_DEPTH`/
/// `MAX_NODES`. Produces one outline line + one [`CachedElement`] per node
/// that has a role or a non-empty label (mirrors the browser snapshot's
/// "interactive or has own text" filter).
fn walk(
    element: &AXUIElement,
    path: &mut Vec<i64>,
    depth: usize,
    lines: &mut Vec<String>,
    elements: &mut Vec<CachedElement>,
) {
    if elements.len() >= MAX_NODES {
        return;
    }
    let (role, label) = role_and_label(element);
    if depth > 0 && (!role.is_empty() || !label.is_empty()) {
        let n = elements.len() + 1;
        let indent = "  ".repeat(depth.saturating_sub(1));
        lines.push(format!("{indent}[{n}] <{role}> \"{label}\""));
        elements.push(CachedElement {
            path: path.clone(),
            role,
            label,
            point: center_point(element),
        });
    }
    if depth >= MAX_DEPTH {
        return;
    }
    for (i, child) in children_of(element).into_iter().enumerate() {
        if elements.len() >= MAX_NODES {
            break;
        }
        path.push(i as i64);
        walk(&child, path, depth + 1, lines, elements);
        path.pop();
    }
}

/// Walk the accessibility tree of the application with process id `pid`.
pub(super) fn walk_app(pid: i32) -> WalkResult {
    let root = unsafe { AXUIElement::new_application(pid as pid_t) };
    let mut path = Vec::new();
    let mut lines = Vec::new();
    let mut elements = Vec::new();
    walk(&root, &mut path, 0, &mut lines, &mut elements);
    WalkResult {
        outline: lines.join("\n"),
        elements,
    }
}

/// Re-derive the live `AXUIElement` at `path` (child indices from the app
/// root), for acting on a ref that was cached during an earlier
/// [`walk_app`]. Returns `NotFound` if any hop along the path is gone
/// (the UI changed since the read — the generation check upstream should
/// normally prevent this, but the tree can also mutate within a
/// generation without a new read, e.g. a list re-sorting).
pub(super) fn resolve_path(
    pid: i32,
    path: &[i64],
) -> Result<CFRetained<AXUIElement>, ComputerError> {
    let mut current = unsafe { AXUIElement::new_application(pid as pid_t) };
    for &idx in path {
        let children = children_of(&current);
        let idx = usize::try_from(idx).map_err(|_| ComputerError::NotFound("bad path".into()))?;
        current = children
            .into_iter()
            .nth(idx)
            .ok_or_else(|| ComputerError::NotFound("element no longer present".into()))?;
    }
    Ok(current)
}

/// Perform an AX action (e.g. `"AXPress"`) on `element`.
pub(super) fn perform_action(element: &AXUIElement, action: &str) -> Result<(), ComputerError> {
    let cf_action = CFString::from_str(action);
    let err = unsafe { element.perform_action(&cf_action) };
    if err != AXError::Success {
        return Err(to_agent_ax_err("perform_action", err));
    }
    Ok(())
}

/// Write `value` directly into `element`'s `AXValue` attribute.
pub(super) fn set_value_string(element: &AXUIElement, value: &str) -> Result<(), ComputerError> {
    let attr = CFString::from_str("AXValue");
    let cf_value = CFString::from_str(value);
    let err = unsafe { element.set_attribute_value(&attr, &cf_value) };
    if err != AXError::Success {
        return Err(to_agent_ax_err("set_value", err));
    }
    Ok(())
}

/// List running applications (name, pid, frontmost).
pub(super) fn list_running_apps() -> Vec<(String, i32, bool)> {
    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost_pid = frontmost_app().map(|(_, pid)| pid);
    workspace
        .runningApplications()
        .to_vec()
        .into_iter()
        .filter_map(|app: objc2::rc::Retained<NSRunningApplication>| {
            let name = app.localizedName()?.to_string();
            let pid = app.processIdentifier();
            Some((name, pid, Some(pid) == frontmost_pid))
        })
        .collect()
}

/// Name and pid of the frontmost application, if resolvable.
pub(super) fn frontmost_app() -> Option<(String, i32)> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let name = app.localizedName()?.to_string();
    let pid = app.processIdentifier();
    Some((name, pid))
}

/// Resolve an app name (case-insensitive) to its pid via the running
/// application list.
pub(super) fn pid_for_app_name(name: &str) -> Option<i32> {
    list_running_apps()
        .into_iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, pid, _)| pid)
}
