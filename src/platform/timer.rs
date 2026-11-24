//! Timers on the main run loop.

use core_foundation_sys::base::CFRelease;
use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
use core_foundation_sys::runloop::{
    CFRunLoopAddTimer, CFRunLoopGetMain, CFRunLoopTimerCreate, CFRunLoopTimerRef, kCFRunLoopCommonModes,
};
use std::ffi::c_void;
use std::time::Duration;

pub fn after(delay: Duration, f: fn()) {
    add(delay, Duration::ZERO, f);
}

pub fn every(interval: Duration, f: fn()) {
    add(Duration::from_millis(100), interval, f);
}

fn add(delay: Duration, interval: Duration, f: fn()) {
    // `info` carries the function itself; a plain fn pointer needs no memory management
    let mut context = core_foundation_sys::runloop::CFRunLoopTimerContext {
        version: 0,
        info: f as *mut c_void,
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: the context is copied. The run loop keeps the timer alive, so our reference
    // can go at once.
    unsafe {
        let fire = CFAbsoluteTimeGetCurrent() + delay.as_secs_f64();
        let t = CFRunLoopTimerCreate(std::ptr::null(), fire, interval.as_secs_f64(), 0, 0, fired, &mut context);
        CFRunLoopAddTimer(CFRunLoopGetMain(), t, kCFRunLoopCommonModes);
        CFRelease(t as *const c_void);
    }
}

extern "C" fn fired(_: CFRunLoopTimerRef, info: *mut c_void) {
    // SAFETY: `info` is the `fn()` that `add` put there.
    let f: fn() = unsafe { std::mem::transmute::<*mut c_void, fn()>(info) };
    f();
}
