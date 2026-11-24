//! Global shortcuts through Carbon, which needs no extra permission.
//! Control-Option-T tiles, Control-Option-L steps through the layouts.

use std::ffi::c_void;

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
    fn InstallEventHandler(target: *mut c_void, h: Handler, n: usize, types: *const EventType, data: *mut c_void, out: *mut *mut c_void) -> i32;
    fn RegisterEventHotKey(code: u32, mods: u32, id: HotKeyId, target: *mut c_void, opts: u32, out: *mut *mut c_void) -> i32;
    fn GetEventParameter(e: *mut c_void, name: u32, ty: u32, actual: *mut u32, size: usize, actual_size: *mut usize, data: *mut c_void) -> i32;
}

const fn four(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
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
    if err == 0 {
        let msg = match id.id {
            1 => crate::app::tile_visible(),
            _ => crate::app::next_layout(),
        };
        crate::app::log(&msg);
    }
    0
}

pub fn install() {
    const CONTROL_OPTION: u32 = 0x1000 | 0x0800;
    unsafe {
        let target = GetApplicationEventTarget();
        let ty = EventType { class: four(b"keyb"), kind: 5 };
        InstallEventHandler(target, pressed, 1, &ty, std::ptr::null_mut(), std::ptr::null_mut());
        for (id, code) in [(1, 17u32), (2, 37)] {
            let mut out = std::ptr::null_mut();
            let err = RegisterEventHotKey(code, CONTROL_OPTION, HotKeyId { signature: four(b"wgrp"), id }, target, 0, &mut out);
            if err != 0 {
                crate::app::log(&format!("shortcut {id} is taken by another app"));
            }
        }
    }
}
