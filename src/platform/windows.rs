//! The window server's list of windows.

use super::screen;
use crate::core::geometry::Rect;
use crate::core::window::{self, Window};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::CFRelease;
use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation_sys::number::{CFNumberGetValue, CFNumberRef, kCFNumberDoubleType};
use std::ffi::c_void;

const ON_SCREEN_ONLY: u32 = 1;
const EXCLUDE_DESKTOP: u32 = 16;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
}

/// Every on-screen window, front to back. Asks the window server, never the apps, so a hung
/// app cannot block it.
pub fn all() -> Vec<Window> {
    // SAFETY: the array is ours to release ("Copy"); the dictionaries in it are borrowed from
    // it and not used after the release.
    unsafe {
        let list = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0);
        if list.is_null() {
            return Vec::new();
        }
        let windows = (0..CFArrayGetCount(list))
            .filter_map(|i| read(CFArrayGetValueAtIndex(list, i) as CFDictionaryRef))
            .collect();
        CFRelease(list as *const c_void);
        windows
    }
}

/// Ordinary on-screen windows, front to back.
pub fn ordinary() -> Vec<Window> {
    window::ordinary(&all())
}

pub fn mission_control_open(all: &[Window]) -> bool {
    window::mission_control_open(all, &screen::bounds())
}

/// SAFETY: `d` must be a live dictionary from the window list.
unsafe fn read(d: CFDictionaryRef) -> Option<Window> {
    unsafe {
        let bounds = value(d, "kCGWindowBounds")? as CFDictionaryRef;
        let side = |key| number(bounds, key).unwrap_or(0.0);
        Some(Window {
            id: number(d, "kCGWindowNumber")? as u32,
            pid: number(d, "kCGWindowOwnerPID")? as i32,
            layer: number(d, "kCGWindowLayer").unwrap_or(0.0) as i32,
            alpha: number(d, "kCGWindowAlpha").unwrap_or(1.0),
            bounds: Rect { x: side("X"), y: side("Y"), w: side("Width"), h: side("Height") },
        })
    }
}

unsafe fn value(d: CFDictionaryRef, key: &str) -> Option<*const c_void> {
    let key = CFString::new(key);
    let v = unsafe { CFDictionaryGetValue(d, key.as_concrete_TypeRef() as *const c_void) };
    (!v.is_null()).then_some(v)
}

unsafe fn number(d: CFDictionaryRef, key: &str) -> Option<f64> {
    let mut out = 0f64;
    let ok = unsafe {
        let v = value(d, key)? as CFNumberRef;
        CFNumberGetValue(v, kCFNumberDoubleType, &mut out as *mut f64 as *mut c_void)
    };
    ok.then_some(out)
}
