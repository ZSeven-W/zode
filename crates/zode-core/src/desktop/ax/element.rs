//! Low-level Core Foundation / Accessibility helpers used only on the AX actor
//! thread. `AxElem` is an RAII wrapper around an `AXUIElementRef` (CFRetain /
//! CFRelease); every raw pointer stays on the owning thread (spec §线程模型).

#![cfg(target_os = "macos")]

use std::os::raw::c_void;

use accessibility_sys::{
    kAXErrorSuccess, AXUIElementCopyAttributeValue, AXUIElementCreateApplication,
    AXUIElementGetTypeID, AXUIElementRef, AXUIElementSetMessagingTimeout, AXValueGetValue,
    AXValueRef,
};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};

/// Owned AX element handle. Not `Send` — never leaves the actor thread.
pub struct AxElem(AXUIElementRef);

impl AxElem {
    /// Wrap a +1 reference returned by an AX copy call (takes ownership).
    ///
    /// # Safety
    /// `p` must be a valid, non-null `AXUIElementRef` owned by the caller.
    pub unsafe fn from_create(p: AXUIElementRef) -> Self {
        AxElem(p)
    }

    /// Retain and wrap a borrowed reference (e.g. an item held by a CFArray).
    ///
    /// # Safety
    /// `p` must be a valid, non-null `AXUIElementRef`.
    pub unsafe fn retain(p: AXUIElementRef) -> Self {
        CFRetain(p as CFTypeRef);
        AxElem(p)
    }

    pub fn as_ref(&self) -> AXUIElementRef {
        self.0
    }
}

/// Create an application AX element for `pid` with a bounded per-call messaging
/// timeout (2s) so a wedged provider surfaces as a timeout instead of hanging
/// the actor thread indefinitely (spec §线程与执行模型).
pub fn create_app(pid: i32) -> AxElem {
    let app = unsafe { AxElem::from_create(AXUIElementCreateApplication(pid)) };
    unsafe { AXUIElementSetMessagingTimeout(app.as_ref(), 2.0) };
    app
}

impl Drop for AxElem {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0 as CFTypeRef) }
    }
}

impl Clone for AxElem {
    fn clone(&self) -> Self {
        unsafe {
            CFRetain(self.0 as CFTypeRef);
            AxElem(self.0)
        }
    }
}

/// Copy a raw attribute value (+1 reference). Returns `None` on any AX error.
///
/// # Safety
/// `elem` must be a valid element on the current thread.
pub unsafe fn copy_attr(elem: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
    let key = CFString::new(attr);
    let mut val: CFTypeRef = std::ptr::null_mut();
    let err = AXUIElementCopyAttributeValue(elem, key.as_concrete_TypeRef(), &mut val);
    if err == kAXErrorSuccess && !val.is_null() {
        Some(val)
    } else {
        None
    }
}

/// Read a string attribute.
///
/// # Safety
/// `elem` must be valid on the current thread.
pub unsafe fn attr_string(elem: AXUIElementRef, attr: &str) -> Option<String> {
    let val = copy_attr(elem, attr)?;
    if CFGetTypeID(val) == CFString::type_id() {
        let s = CFString::wrap_under_create_rule(val as CFStringRef);
        Some(s.to_string())
    } else {
        CFRelease(val);
        None
    }
}

/// Read an array-of-elements attribute (e.g. `AXChildren`, `AXWindows`).
///
/// # Safety
/// `elem` must be valid on the current thread.
pub unsafe fn attr_elem_array(elem: AXUIElementRef, attr: &str) -> Vec<AxElem> {
    let Some(val) = copy_attr(elem, attr) else {
        return Vec::new();
    };
    if CFGetTypeID(val) != CFArray::<*const c_void>::type_id() {
        CFRelease(val);
        return Vec::new();
    }
    let arr: CFArray<*const c_void> = CFArray::wrap_under_create_rule(val as CFArrayRef);
    let mut out = Vec::with_capacity(arr.len() as usize);
    for item in arr.iter() {
        let p = *item as AXUIElementRef;
        if !p.is_null() && CFGetTypeID(p as CFTypeRef) == AXUIElementGetTypeID() {
            out.push(AxElem::retain(p));
        }
    }
    out
}

/// Read a `CGPoint`-valued AXValue attribute (e.g. `AXPosition`).
///
/// # Safety
/// `elem` must be valid on the current thread.
pub unsafe fn attr_point(elem: AXUIElementRef, attr: &str) -> Option<CGPoint> {
    let val = copy_attr(elem, attr)?;
    let mut pt = CGPoint { x: 0.0, y: 0.0 };
    let ok = AXValueGetValue(
        val as AXValueRef,
        accessibility_sys::kAXValueTypeCGPoint,
        &mut pt as *mut _ as *mut c_void,
    );
    CFRelease(val);
    ok.then_some(pt)
}

/// Read a `CGSize`-valued AXValue attribute (e.g. `AXSize`).
///
/// # Safety
/// `elem` must be valid on the current thread.
pub unsafe fn attr_size(elem: AXUIElementRef, attr: &str) -> Option<CGSize> {
    let val = copy_attr(elem, attr)?;
    let mut sz = CGSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = AXValueGetValue(
        val as AXValueRef,
        accessibility_sys::kAXValueTypeCGSize,
        &mut sz as *mut _ as *mut c_void,
    );
    CFRelease(val);
    ok.then_some(sz)
}
