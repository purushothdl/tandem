//! The Accessibility API: reading, moving and watching other apps' windows.
//!
//! These calls are messages to another process. They take time, and fail or time out when
//! the app is busy.

use super::{CGPoint, CGSize};
use crate::core::geometry::Rect;
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::runloop::{CFRunLoop, CFRunLoopSource, CFRunLoopSourceRef, kCFRunLoopCommonModes};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::array::CFArrayRef;
use std::ffi::c_void;

type RawElement = *const c_void;
type AXError = i32;
type Callback = extern "C" fn(obs: *mut c_void, el: RawElement, notif: CFStringRef, refcon: *mut c_void);

const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;
const TIMEOUT_SECONDS: f32 = 0.4;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> RawElement;
    fn AXUIElementCopyAttributeValue(el: RawElement, attr: CFStringRef, value: *mut CFTypeRef) -> AXError;
    fn AXUIElementCopyMultipleAttributeValues(
        el: RawElement,
        names: CFArrayRef,
        options: u32,
        values: *mut CFArrayRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(el: RawElement, attr: CFStringRef, value: CFTypeRef) -> AXError;
    fn AXUIElementPerformAction(el: RawElement, action: CFStringRef) -> AXError;
    fn AXUIElementSetMessagingTimeout(el: RawElement, seconds: f32) -> AXError;
    fn AXValueCreate(kind: u32, ptr: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(v: CFTypeRef, kind: u32, ptr: *mut c_void) -> bool;
    fn AXObserverCreate(pid: i32, cb: Callback, out: *mut CFTypeRef) -> AXError;
    fn AXObserverAddNotification(obs: CFTypeRef, el: RawElement, notif: CFStringRef, refcon: *mut c_void) -> AXError;
    fn AXObserverGetRunLoopSource(obs: CFTypeRef) -> CFRunLoopSourceRef;
    // Private but stable for years; yabai and AeroSpace rely on it.
    fn _AXUIElementGetWindow(el: RawElement, out: *mut u32) -> AXError;
}

/// May we control other apps? `prompt`: let macOS ask the user.
pub fn is_trusted(prompt: bool) -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let val = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
}

/// A window or app, as an owned AXUIElement: retained on clone, released on drop.
#[derive(Clone)]
pub struct Element(CFType);

// SAFETY: accessibility elements may be messaged from any thread, and their reference count
// is atomic.
unsafe impl Send for Element {}

impl Element {
    pub fn app(pid: i32) -> Option<Element> {
        let p = unsafe { AXUIElementCreateApplication(pid) };
        if p.is_null() {
            return None;
        }
        let app = Element(unsafe { CFType::wrap_under_create_rule(p as CFTypeRef) });
        app.set_timeout();
        Some(app)
    }

    fn raw(&self) -> RawElement {
        self.0.as_CFTypeRef() as RawElement
    }

    pub fn same(&self, o: &Element) -> bool {
        self.0 == o.0
    }

    fn attr(&self, name: &str) -> Option<CFType> {
        let mut out: CFTypeRef = std::ptr::null();
        let key = CFString::new(name);
        let err = unsafe { AXUIElementCopyAttributeValue(self.raw(), key.as_concrete_TypeRef(), &mut out) };
        (err == 0 && !out.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn window_id(&self) -> Option<u32> {
        let mut id = 0u32;
        (unsafe { _AXUIElementGetWindow(self.raw(), &mut id) } == 0 && id != 0).then_some(id)
    }

    /// Make the owning app the active one (call on an application element).
    pub fn make_frontmost(&self) {
        self.set_attr("AXFrontmost", CFBoolean::true_value().as_CFTypeRef());
    }

    pub fn is_minimized(&self) -> bool {
        self.attr("AXMinimized").and_then(|v| v.downcast::<CFBoolean>()).is_some_and(|v| v == CFBoolean::true_value())
    }

    pub fn title(&self) -> Option<String> {
        self.attr("AXTitle").and_then(|v| v.downcast::<CFString>()).map(|v| v.to_string()).filter(|t| !t.is_empty())
    }

    /// A normal document window, not a dialog, palette or sheet.
    pub fn is_standard_window(&self) -> bool {
        self.attr("AXSubrole").and_then(|v| v.downcast::<CFString>()).is_some_and(|v| v == "AXStandardWindow")
    }

    /// Position and size in one round trip; two calls per read made polling slow.
    pub fn frame(&self) -> Option<Rect> {
        let names = CFArray::from_CFTypes(&[CFString::new("AXPosition"), CFString::new("AXSize")]);
        let mut out: CFArrayRef = std::ptr::null();
        let err =
            unsafe { AXUIElementCopyMultipleAttributeValues(self.raw(), names.as_concrete_TypeRef(), 0, &mut out) };
        if err != 0 || out.is_null() {
            return None;
        }
        let arr: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(out) };
        let (pos, size) = (arr.get(0)?, arr.get(1)?);
        let mut p = CGPoint::default();
        let mut s = CGSize::default();
        let ok = unsafe {
            AXValueGetValue(pos.as_CFTypeRef(), AX_POINT, &mut p as *mut _ as *mut c_void)
                && AXValueGetValue(size.as_CFTypeRef(), AX_SIZE, &mut s as *mut _ as *mut c_void)
        };
        ok.then_some(Rect { x: p.x, y: p.y, w: s.w, h: s.h })
    }

    fn set_attr(&self, name: &str, value: CFTypeRef) -> bool {
        let key = CFString::new(name);
        unsafe { AXUIElementSetAttributeValue(self.raw(), key.as_concrete_TypeRef(), value) == 0 }
    }

    pub fn set_position(&self, x: f64, y: f64) {
        let p = CGPoint { x, y };
        unsafe {
            let pv = CFType::wrap_under_create_rule(AXValueCreate(AX_POINT, &p as *const _ as *const c_void));
            self.set_attr("AXPosition", pv.as_CFTypeRef());
        }
    }

    pub fn set_size(&self, w: f64, h: f64) {
        let s = CGSize { w, h };
        unsafe {
            let sv = CFType::wrap_under_create_rule(AXValueCreate(AX_SIZE, &s as *const _ as *const c_void));
            self.set_attr("AXSize", sv.as_CFTypeRef());
        }
    }

    /// Give up on a hung app soon instead of freezing with it.
    fn set_timeout(&self) {
        unsafe { AXUIElementSetMessagingTimeout(self.raw(), TIMEOUT_SECONDS) };
    }

    /// In front of its app's other windows, without activating the app.
    pub fn raise(&self) {
        let action = CFString::new("AXRaise");
        unsafe { AXUIElementPerformAction(self.raw(), action.as_concrete_TypeRef()) };
    }
}

/// The element for a window we know from the window list.
pub fn find_window(pid: i32, wid: u32) -> Option<Element> {
    let list = Element::app(pid)?.attr("AXWindows")?;
    // SAFETY: AXWindows is documented to be an array of elements.
    let windows = unsafe { CFArray::<CFType>::wrap_under_get_rule(list.as_CFTypeRef() as CFArrayRef) };
    let found = windows.iter().map(|w| Element((*w).clone())).find(|w| w.window_id() == Some(wid))?;
    found.set_timeout();
    Some(found)
}

// ---- notifications ----

/// What can happen to a window we watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Closed,
    Minimized,
    Restored,
}

const CHANGES: [(&str, Change); 3] = [
    ("AXUIElementDestroyed", Change::Closed),
    ("AXWindowMiniaturized", Change::Minimized),
    ("AXWindowDeminiaturized", Change::Restored),
];

/// Reports changes to windows of one app, on the main thread.
pub struct Observer {
    raw: CFType,
    handler: fn(Change, &Element),
}

impl Observer {
    pub fn new(pid: i32, handler: fn(Change, &Element)) -> Option<Observer> {
        let mut out: CFTypeRef = std::ptr::null();
        if unsafe { AXObserverCreate(pid, notified, &mut out) } != 0 || out.is_null() {
            return None;
        }
        // SAFETY: ours to release ("Create"); the run loop source is borrowed ("Get").
        let obs = unsafe { CFType::wrap_under_create_rule(out) };
        let source = unsafe { CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(obs.as_CFTypeRef())) };
        CFRunLoop::get_main().add_source(&source, unsafe { kCFRunLoopCommonModes });
        Some(Observer { raw: obs, handler })
    }

    /// Watch one of the app's windows.
    pub fn watch(&self, window: &Element) {
        for (name, _) in CHANGES {
            let n = CFString::new(name);
            // the handler itself travels as the callback's context
            let refcon = self.handler as *mut c_void;
            unsafe {
                AXObserverAddNotification(self.raw.as_CFTypeRef(), window.raw(), n.as_concrete_TypeRef(), refcon);
            }
        }
    }
}

extern "C" fn notified(_: *mut c_void, el: RawElement, name: CFStringRef, refcon: *mut c_void) {
    // SAFETY: the arguments are borrowed for the call; `refcon` is the handler set in `watch`.
    let (name, window, handler) = unsafe {
        (
            CFString::wrap_under_get_rule(name).to_string(),
            Element(CFType::wrap_under_get_rule(el as CFTypeRef)),
            std::mem::transmute::<*mut c_void, fn(Change, &Element)>(refcon),
        )
    };
    if let Some((_, change)) = CHANGES.iter().find(|(n, _)| *n == name) {
        handler(*change, &window);
    }
}
