//! One thread per window, the only one that moves it.
//!
//! Frames come through a channel. Moving a window is a slow call into another process, so
//! frames can arrive faster than they can be carried out; the thread then skips to the newest.
//! A slow app delays nobody but itself.

use crate::core::geometry::{Axis, Rect};
use crate::core::refusal::{Minimum, Refusals};
use crate::platform::element::Element;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// How often to ask an app for the same frame.
const TRIES: u32 = 14;
/// After so many tries with nothing new to do, a refusal is the app's own.
const SETTLED: u32 = 9;
/// Quick at first, then patient.
const QUICK_TRIES: u32 = 4;
const QUICK: Duration = Duration::from_millis(12);
const PATIENT: Duration = Duration::from_millis(70);

/// Found by the threads, waiting for the main one.
static MINIMUMS: Mutex<Vec<(u32, Axis, Minimum)>> = Mutex::new(Vec::new());

pub fn take_minimums() -> Vec<(u32, Axis, Minimum)> {
    std::mem::take(&mut *MINIMUMS.lock().unwrap())
}

/// The thread ends when the last sender is dropped.
pub fn spawn(element: Element, wid: u32) -> Sender<Rect> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run(&element, wid, &rx));
    tx
}

fn run(element: &Element, wid: u32, frames: &Receiver<Rect>) {
    let mut refusals = Refusals::default();
    while let Ok(first) = frames.recv() {
        let mut target = frames.try_iter().last().unwrap_or(first);
        let Some(mut now) = element.frame() else { continue };
        let mut tries = 0;
        while !same(&now, &target) && tries < TRIES {
            let Some(got) = place(element, now, target) else { break };
            now = got;
            let newer = frames.try_iter().last();
            let settled = newer.is_none() && tries >= SETTLED;
            for (axis, asked, got) in [(Axis::X, target.w, now.w), (Axis::Y, target.h, now.h)] {
                if let Some(min) = refusals.observe(axis, asked, got, settled, Instant::now()) {
                    MINIMUMS.lock().unwrap().push((wid, axis, min));
                }
            }
            if let Some(newer) = newer {
                target = newer;
                tries = 0;
                continue;
            }
            tries += 1;
            if !same(&now, &target) {
                std::thread::sleep(if tries < QUICK_TRIES { QUICK } else { PATIENT });
                now = element.frame().unwrap_or(now);
            }
        }
    }
}

fn same(a: &Rect, b: &Rect) -> bool {
    a.distance(b) < 1.0
}

/// Size first, then position. If the app refuses to shrink, hold the edge that was supposed
/// to stay put, so it never slides out of its corner of the group. Returns where it ended up.
fn place(element: &Element, now: Rect, to: Rect) -> Option<Rect> {
    let mut got = now;
    if (now.w - to.w).abs() > 0.5 || (now.h - to.h).abs() > 0.5 {
        element.set_size(to.w, to.h);
        got = element.frame()?;
    }
    let held = |axis: Axis| {
        let (start, to_start, size, want) = (now.lo(axis), to.lo(axis), got.length(axis), to.length(axis));
        let too_big = size > want + 2.0;
        if to_start - start > 0.5 && too_big { to_start + want - size } else { to_start }
    };
    let (x, y) = (held(Axis::X), held(Axis::Y));
    if (got.x - x).abs() > 0.5 || (got.y - y).abs() > 0.5 {
        element.set_position(x, y);
        got = element.frame()?;
    }
    Some(got)
}
