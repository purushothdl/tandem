//! Thin wrapper over the macOS Accessibility API (AXUIElement / AXObserver).

use crate::layout::Rect;
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopSource, CFRunLoopSourceRef};
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

pub type RawElem = *const c_void;
type AXError = i32;
pub type Callback = extern "C" fn(obs: *mut c_void, el: RawElem, notif: CFStringRef, refcon: *mut c_void);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGSize {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> RawElem;
    fn AXUIElementCopyAttributeValue(el: RawElem, attr: CFStringRef, value: *mut CFTypeRef) -> AXError;
    fn AXUIElementSetAttributeValue(el: RawElem, attr: CFStringRef, value: CFTypeRef) -> AXError;
    fn AXUIElementPerformAction(el: RawElem, action: CFStringRef) -> AXError;
    fn AXValueCreate(kind: u32, ptr: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(v: CFTypeRef, kind: u32, ptr: *mut c_void) -> bool;
    fn AXObserverCreate(pid: i32, cb: Callback, out: *mut CFTypeRef) -> AXError;
    fn AXObserverAddNotification(obs: CFTypeRef, el: RawElem, notif: CFStringRef, refcon: *mut c_void) -> AXError;
    fn AXObserverGetRunLoopSource(obs: CFTypeRef) -> CFRunLoopSourceRef;
    // Private but stable for years; yabai and AeroSpace rely on it.
    fn _AXUIElementGetWindow(el: RawElem, out: *mut u32) -> AXError;
    fn AXUIElementSetMessagingTimeout(el: RawElem, seconds: f32) -> AXError;
    fn AXUIElementCopyMultipleAttributeValues(
        el: RawElem,
        names: core_foundation_sys::array::CFArrayRef,
        options: u32,
        values: *mut core_foundation_sys::array::CFArrayRef,
    ) -> AXError;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(id: u32) -> CGRect;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> core_foundation_sys::array::CFArrayRef;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        cb: TapCallback,
        info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
}

pub type TapCallback = extern "C" fn(proxy: *mut c_void, kind: u32, event: *mut c_void, info: *mut c_void) -> *mut c_void;

static TAP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Watch left mouse presses (needs the Accessibility permission we already hold).
/// The callback runs *before* the click reaches the app, which lets us raise a
/// window group first so everything appears together.
pub fn install_click_tap(cb: TapCallback) -> bool {
    use core_foundation_sys::mach_port::CFMachPortCreateRunLoopSource;
    use core_foundation_sys::runloop::{CFRunLoopAddSource, CFRunLoopGetMain};
    unsafe {
        // session tap, head insertion, active (not listen-only): left mouse down, up, dragged
        let tap = CGEventTapCreate(1, 0, 0, (1u64 << 1) | (1u64 << 2) | (1u64 << 6), cb, std::ptr::null_mut());
        if tap.is_null() {
            return false;
        }
        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap as _, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        TAP.store(tap as usize, std::sync::atomic::Ordering::Relaxed);
    }
    true
}

/// macOS turns a slow tap off; turn it back on.
pub fn reenable_tap() {
    let t = TAP.load(std::sync::atomic::Ordering::Relaxed);
    if t != 0 {
        unsafe { CGEventTapEnable(t as *mut c_void, true) };
    }
}

pub fn event_location(event: *mut c_void) -> (f64, f64) {
    let p = unsafe { CGEventGetLocation(event) };
    (p.x, p.y)
}

pub fn shift_held(event: *mut c_void) -> bool {
    unsafe { CGEventGetFlags(event) & (1 << 17) != 0 }
}

pub fn click_count(event: *mut c_void) -> i64 {
    unsafe { CGEventGetIntegerValueField(event, 1) }
}

/// Is the left mouse button held down right now? (No permission needed.)
pub fn left_button_down() -> bool {
    unsafe { CGEventSourceButtonState(0, 0) }
}

#[derive(Clone, Copy, Debug)]
pub struct WinInfo {
    pub id: u32,
    pub pid: i32,
    pub layer: i32,
    pub alpha: f64,
    pub bounds: Rect,
}

/// Normal on-screen windows, front to back.
pub fn window_order() -> Vec<WinInfo> {
    normal(&all_windows())
}

/// The ordinary windows of a list; invisible helper windows (Electron has some) do not count.
pub fn normal(all: &[WinInfo]) -> Vec<WinInfo> {
    // the Dock (owner of layer 20) adds helper windows of its own during Mission Control
    let dock = all.iter().find(|w| w.layer == 20).map(|w| w.pid);
    all.iter().filter(|w| w.layer == 0 && w.alpha > 0.05 && Some(w.pid) != dock).copied().collect()
}

/// Every on-screen window, front to back. Asks the window server, never the apps,
/// so a hung app cannot block it.
pub fn all_windows() -> Vec<WinInfo> {
    use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex};
    use core_foundation_sys::base::CFRelease;
    use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
    use core_foundation_sys::number::{kCFNumberDoubleType, CFNumberGetValue, CFNumberRef};

    fn num(d: CFDictionaryRef, key: &str) -> Option<f64> {
        let k = CFString::new(key);
        let v = unsafe { CFDictionaryGetValue(d, k.as_concrete_TypeRef() as *const c_void) };
        if v.is_null() {
            return None;
        }
        let mut out = 0f64;
        unsafe { CFNumberGetValue(v as CFNumberRef, kCFNumberDoubleType, &mut out as *mut f64 as *mut c_void) }.then_some(out)
    }

    let mut out = Vec::new();
    unsafe {
        // OnScreenOnly | ExcludeDesktopElements
        let arr = CGWindowListCopyWindowInfo(1 | 16, 0);
        if arr.is_null() {
            return out;
        }
        for i in 0..CFArrayGetCount(arr) {
            let d = CFArrayGetValueAtIndex(arr, i) as CFDictionaryRef;
            let (Some(id), Some(pid)) = (num(d, "kCGWindowNumber"), num(d, "kCGWindowOwnerPID")) else { continue };
            let layer = num(d, "kCGWindowLayer").unwrap_or(0.0) as i32;
            let alpha = num(d, "kCGWindowAlpha").unwrap_or(1.0);
            let bk = CFString::new("kCGWindowBounds");
            let b = CFDictionaryGetValue(d, bk.as_concrete_TypeRef() as *const c_void) as CFDictionaryRef;
            if b.is_null() {
                continue;
            }
            let bounds = Rect {
                x: num(b, "X").unwrap_or(0.0),
                y: num(b, "Y").unwrap_or(0.0),
                w: num(b, "Width").unwrap_or(0.0),
                h: num(b, "Height").unwrap_or(0.0),
            };
            out.push(WinInfo { id: id as u32, pid: pid as i32, layer, alpha, bounds });
        }
        CFRelease(arr as *const c_void);
    }
    out
}

pub fn main_display_bounds() -> Rect {
    let b = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    Rect { x: b.origin.x, y: b.origin.y, w: b.size.w, h: b.size.h }
}

pub fn is_trusted(prompt: bool) -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let val = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
}

type Psn = [u32; 2];
struct Slps {
    get: unsafe extern "C" fn(i32, *mut Psn) -> i32,
    front: unsafe extern "C" fn(*const Psn, u32, u32) -> i32,
    post: unsafe extern "C" fn(*const Psn, *const u8) -> i32,
}

/// Private calls for giving a window the keyboard without moving it (as yabai does).
fn slps() -> Option<&'static Slps> {
    static FNS: std::sync::OnceLock<Option<Slps>> = std::sync::OnceLock::new();
    unsafe extern "C" {
        fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut c_void;
        fn dlsym(h: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
    }
    FNS.get_or_init(|| unsafe {
        let h = dlopen(c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(), 1);
        if h.is_null() {
            return None;
        }
        let get = dlsym(-2isize as *mut c_void, c"GetProcessForPID".as_ptr());
        let front = dlsym(h, c"_SLPSSetFrontProcessWithOptions".as_ptr());
        let post = dlsym(h, c"SLPSPostEventRecordTo".as_ptr());
        if get.is_null() || front.is_null() || post.is_null() {
            return None;
        }
        Some(Slps { get: std::mem::transmute(get), front: std::mem::transmute(front), post: std::mem::transmute(post) })
    })
    .as_ref()
}

/// Give a window the keyboard and leave it where it is in the stack.
pub fn focus_without_raising(pid: i32, wid: u32) -> bool {
    let Some(f) = slps() else { return false };
    let mut psn: Psn = [0; 2];
    unsafe {
        if (f.get)(pid, &mut psn) != 0 || (f.front)(&psn, wid, 0x400) != 0 {
            return false;
        }
        for step in [1u8, 2] {
            let mut rec = [0u8; 0xf8];
            rec[0x04] = 0xf8;
            rec[0x08] = step;
            rec[0x3a] = 0x10;
            rec[0x3c..0x40].copy_from_slice(&wid.to_ne_bytes());
            rec[0x20..0x30].fill(0xff);
            (f.post)(&psn, rec.as_ptr());
        }
    }
    true
}

/// Mission Control and App Exposé put a full-screen Dock window on layer 18 and shrink
/// everything in the window list, so positions mean nothing while it is open.
pub fn mission_control_open(all: &[WinInfo]) -> bool {
    let d = main_display_bounds();
    all.iter().any(|w| w.layer == 18 && w.bounds.w >= d.w && w.bounds.h >= d.h)
}

/// An owned AXUIElement.
#[derive(Clone)]
pub struct Elem(CFType);

impl Elem {
    pub fn app(pid: i32) -> Option<Elem> {
        let p = unsafe { AXUIElementCreateApplication(pid) };
        (!p.is_null()).then(|| Elem(unsafe { CFType::wrap_under_create_rule(p as CFTypeRef) }))
    }

    /// For callback arguments, which are borrowed.
    pub fn from_borrowed(p: RawElem) -> Elem {
        Elem(unsafe { CFType::wrap_under_get_rule(p as CFTypeRef) })
    }

    fn raw(&self) -> RawElem {
        self.0.as_CFTypeRef() as RawElem
    }

    pub fn same(&self, o: &Elem) -> bool {
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

    /// Position and size in one round trip (two calls per read is what made polling slow).
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
        self.attr("AXSubrole")
            .and_then(|v| v.downcast::<CFString>())
            .is_some_and(|v| v == "AXStandardWindow")
    }

    pub fn frame(&self) -> Option<Rect> {
        use core_foundation::array::CFArray;
        let names = CFArray::from_CFTypes(&[CFString::new("AXPosition"), CFString::new("AXSize")]);
        let mut out: core_foundation_sys::array::CFArrayRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyMultipleAttributeValues(self.raw(), names.as_concrete_TypeRef(), 0, &mut out) };
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

    /// Give up on a hung app after this long instead of freezing wingroup.
    pub fn set_timeout(&self, seconds: f32) {
        unsafe { AXUIElementSetMessagingTimeout(self.raw(), seconds) };
    }

    pub fn raise(&self) {
        let action = CFString::new("AXRaise");
        unsafe { AXUIElementPerformAction(self.raw(), action.as_concrete_TypeRef()) };
    }
}

/// The accessibility element for a window we know from the window list.
pub fn find_window(pid: i32, wid: u32) -> Option<Elem> {
    let app = Elem::app(pid)?;
    app.set_timeout(0.4);
    let list = app.attr("AXWindows")?;
    let arr = unsafe {
        core_foundation::array::CFArray::<CFType>::wrap_under_get_rule(list.as_CFTypeRef() as core_foundation_sys::array::CFArrayRef)
    };
    let found = arr.iter().map(|w| Elem((*w).clone())).find(|w| w.window_id() == Some(wid));
    found
}

pub struct Observer(CFType);

impl Observer {
    pub fn new(pid: i32, cb: Callback) -> Option<Observer> {
        let mut out: CFTypeRef = std::ptr::null();
        if unsafe { AXObserverCreate(pid, cb, &mut out) } != 0 || out.is_null() {
            return None;
        }
        let obs = unsafe { CFType::wrap_under_create_rule(out) };
        let src = unsafe { CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(obs.as_CFTypeRef())) };
        CFRunLoop::get_main().add_source(&src, unsafe { kCFRunLoopCommonModes });
        Some(Observer(obs))
    }

    pub fn add(&self, el: &Elem, notif: &str) -> bool {
        let n = CFString::new(notif);
        unsafe { AXObserverAddNotification(self.0.as_CFTypeRef(), el.raw(), n.as_concrete_TypeRef(), std::ptr::null_mut()) == 0 }
    }
}
