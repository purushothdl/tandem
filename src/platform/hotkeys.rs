//! Global shortcuts through Carbon, which needs no extra permission.

use super::log::log;
use std::ffi::c_void;
use std::sync::OnceLock;

#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}
#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
type Handler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        h: Handler,
        n: usize,
        types: *const EventType,
        data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        mods: u32,
        id: HotKeyId,
        target: *mut c_void,
        opts: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn GetEventParameter(
        e: *mut c_void,
        name: u32,
        ty: u32,
        actual: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

const fn four(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const CONTROL: u32 = 0x1000;
const OPTION: u32 = 0x0800;
const KEY_PRESSED: u32 = 5;

/// Virtual key codes.
pub const KEY_T: u32 = 17;
pub const KEY_L: u32 = 37;

static ACTIONS: OnceLock<Vec<fn()>> = OnceLock::new();

/// Control-Option with each of these keys.
pub fn install(shortcuts: &[(u32, fn())]) {
    if ACTIONS.set(shortcuts.iter().map(|s| s.1).collect()).is_err() {
        return;
    }
    // SAFETY: the handler and the shortcuts stay for the rest of the program.
    unsafe {
        let target = GetApplicationEventTarget();
        let ty = EventType { class: four(b"keyb"), kind: KEY_PRESSED };
        InstallEventHandler(target, pressed, 1, &ty, std::ptr::null_mut(), std::ptr::null_mut());
        for (i, (code, _)) in shortcuts.iter().enumerate() {
            let id = HotKeyId { signature: four(b"wgrp"), id: i as u32 };
            let mut out = std::ptr::null_mut();
            if RegisterEventHotKey(*code, CONTROL | OPTION, id, target, 0, &mut out) != 0 {
                log(&format!("shortcut {i} is taken by another app"));
            }
        }
    }
}

extern "C" fn pressed(_next: *mut c_void, event: *mut c_void, _data: *mut c_void) -> i32 {
    let mut id = HotKeyId { signature: 0, id: 0 };
    let err = unsafe {
        GetEventParameter(
            event,
            four(b"----"),
            four(b"hkid"),
            std::ptr::null_mut(),
            std::mem::size_of::<HotKeyId>(),
            std::ptr::null_mut(),
            &mut id as *mut _ as *mut c_void,
        )
    };
    let action = ACTIONS.get().and_then(|a| a.get(id.id as usize));
    if let Some(f) = action.filter(|_| err == 0) {
        f();
    }
    0
}
