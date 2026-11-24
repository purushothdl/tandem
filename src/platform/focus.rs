//! Giving a window the keyboard.

use super::element::Element;
use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

/// Make a window's app the active one, with that window in front.
pub fn activate(pid: i32, window: &Element) {
    window.raise();
    if let Some(app) = Element::app(pid) {
        app.make_frontmost();
    }
}

// ---- private calls ----

type Psn = [u32; 2];
type PsnForPid = unsafe extern "C" fn(i32, *mut Psn) -> i32;
type SetFront = unsafe extern "C" fn(*const Psn, u32, u32) -> i32;
type PostRecord = unsafe extern "C" fn(*const Psn, *const u8) -> i32;

struct Calls {
    psn_for_pid: PsnForPid,
    set_front: SetFront,
    post_record: PostRecord,
}

const RTLD_LAZY: i32 = 1;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
const USER_GENERATED_NO_WINDOWS: u32 = 0x400;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(h: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// Looked up at run time, so a macOS without them still starts wingroup.
fn calls() -> Option<&'static Calls> {
    static FNS: OnceLock<Option<Calls>> = OnceLock::new();
    // SAFETY: each symbol is cast to the signature it is known to have; a missing one is
    // caught as null and turns the feature off.
    FNS.get_or_init(|| unsafe {
        let h = dlopen(c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(), RTLD_LAZY);
        if h.is_null() {
            return None;
        }
        let get = dlsym(RTLD_DEFAULT, c"GetProcessForPID".as_ptr());
        let front = dlsym(h, c"_SLPSSetFrontProcessWithOptions".as_ptr());
        let post = dlsym(h, c"SLPSPostEventRecordTo".as_ptr());
        if get.is_null() || front.is_null() || post.is_null() {
            return None;
        }
        Some(Calls {
            psn_for_pid: std::mem::transmute::<*mut c_void, PsnForPid>(get),
            set_front: std::mem::transmute::<*mut c_void, SetFront>(front),
            post_record: std::mem::transmute::<*mut c_void, PostRecord>(post),
        })
    })
    .as_ref()
}

/// Give a window the keyboard and leave it where it is in the stack.
pub fn focus_without_raising(pid: i32, wid: u32) -> bool {
    let Some(f) = calls() else { return false };
    let mut psn: Psn = [0; 2];
    unsafe {
        if (f.psn_for_pid)(pid, &mut psn) != 0 || (f.set_front)(&psn, wid, USER_GENERATED_NO_WINDOWS) != 0 {
            return false;
        }
        // a pair of synthetic events makes the window key; the layout was found by
        // reverse engineering (yabai uses the same)
        for step in [1u8, 2] {
            let mut rec = [0u8; 0xf8];
            rec[0x04] = 0xf8;
            rec[0x08] = step;
            rec[0x3a] = 0x10;
            rec[0x3c..0x40].copy_from_slice(&wid.to_ne_bytes());
            rec[0x20..0x30].fill(0xff);
            (f.post_record)(&psn, rec.as_ptr());
        }
    }
    true
}
