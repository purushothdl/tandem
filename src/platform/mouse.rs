//! The mouse: an event tap on the left button, and its raw state.

use super::CGPoint;
use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
use core_foundation_sys::runloop::{CFRunLoopAddSource, CFRunLoopGetMain, kCFRunLoopCommonModes};
use objc2_app_kit::NSEvent;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

type TapCallback = extern "C" fn(proxy: *mut c_void, kind: u32, event: *mut c_void, info: *mut c_void) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
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
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
}

const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const ACTIVE: u32 = 0;

const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const LEFT_DRAGGED: u32 = 6;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

const CLICK_STATE: u32 = 1;
const SHIFT: u64 = 1 << 17;

const COMBINED_STATE: i32 = 0;
const HID_STATE: i32 = 1;
const LEFT: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Down,
    Dragged,
    Up,
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    pub clicks: i64,
    pub shift: bool,
}

/// What becomes of an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not ours: on to the app.
    Pass,
    /// Ours; no app sees it.
    Swallow,
}

pub struct Handlers {
    pub event: fn(Event) -> Verdict,
    /// The tap was off for a moment, so a button release may have been missed.
    pub interrupted: fn(),
}

static HANDLERS: OnceLock<Handlers> = OnceLock::new();
static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Watch the left button. The handler runs *before* the click reaches the app, so a whole
/// group can be raised first. Needs the Accessibility permission.
pub fn watch(handlers: Handlers) -> bool {
    if HANDLERS.set(handlers).is_err() {
        return false;
    }
    let events = [LEFT_DOWN, LEFT_UP, LEFT_DRAGGED].iter().fold(0u64, |mask, k| mask | 1 << k);
    // SAFETY: the tap and its source live for the rest of the program.
    unsafe {
        let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, ACTIVE, events, tapped, std::ptr::null_mut());
        if tap.is_null() {
            return false;
        }
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap as CFMachPortRef, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        TAP.store(tap, Ordering::Relaxed);
    }
    true
}

extern "C" fn tapped(_proxy: *mut c_void, kind: u32, event: *mut c_void, _info: *mut c_void) -> *mut c_void {
    let Some(handlers) = HANDLERS.get() else { return event };
    let kind = match kind {
        LEFT_DOWN => Kind::Down,
        LEFT_DRAGGED => Kind::Dragged,
        LEFT_UP => Kind::Up,
        // macOS turns a slow tap off; turn it back on
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER => {
            (handlers.interrupted)();
            unsafe { CGEventTapEnable(TAP.load(Ordering::Relaxed), true) };
            return event;
        }
        _ => return event,
    };
    // SAFETY: the event is valid for the duration of the callback.
    let e = unsafe {
        let p = CGEventGetLocation(event);
        Event {
            kind,
            x: p.x,
            y: p.y,
            clicks: CGEventGetIntegerValueField(event, CLICK_STATE),
            shift: CGEventGetFlags(event) & SHIFT != 0,
        }
    };
    match (handlers.event)(e) {
        Verdict::Pass => event,
        Verdict::Swallow => std::ptr::null_mut(),
    }
}

/// Is the left button down right now?
pub fn left_down() -> bool {
    unsafe { CGEventSourceButtonState(COMBINED_STATE, LEFT) }
}

/// Notices presses by looking, for where no events reach us. Uses the hardware's count as
/// well as the button's state, so a quick tap between two looks is not missed.
#[derive(Default)]
pub struct PressWatch {
    count: u32,
    down: bool,
}

impl PressWatch {
    fn now() -> PressWatch {
        let count = unsafe { CGEventSourceCounterForEventType(HID_STATE, LEFT_DOWN) };
        PressWatch { count, down: NSEvent::pressedMouseButtons() & 1 != 0 }
    }

    fn differs(&self, now: &PressWatch) -> bool {
        now.count != self.count || (now.down && !self.down)
    }

    /// Was there a press since the last `take`?
    pub fn peek(&self) -> bool {
        self.differs(&Self::now())
    }

    /// The same, and start again. One sample for both: a press between two would be lost.
    pub fn take(&mut self) -> bool {
        let now = Self::now();
        let pressed = self.differs(&now);
        *self = now;
        pressed
    }
}
