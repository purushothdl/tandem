//! One thread per window that moves it. Only these threads write frames, so a slow
//! app delays nobody but itself, and the newest target always wins.

use crate::ax::Elem;
use crate::layout::Rect;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// A size an app would not go below, waiting for the main thread.
pub struct Minimum {
    pub wid: u32,
    pub width: bool,
    pub size: f64,
    /// It kept refusing with nothing else going on. Otherwise it only looks that way, which
    /// is good enough to stop a seam being dragged but must not be remembered.
    pub sure: bool,
}

static MINIMUMS: Mutex<Vec<Minimum>> = Mutex::new(Vec::new());

pub fn take_minimums() -> Vec<Minimum> {
    std::mem::take(&mut *MINIMUMS.lock().unwrap())
}

pub struct SendElem(pub Elem);
// Accessibility elements may be messaged from any thread.
unsafe impl Send for SendElem {}

pub fn spawn(elem: Elem, wid: u32) -> Sender<Rect> {
    let (tx, rx) = channel();
    let el = SendElem(elem);
    std::thread::spawn(move || {
        let el = el;
        run(&el.0, wid, &rx)
    });
    tx
}

fn close(a: &Rect, b: &Rect) -> bool {
    a.distance(b) < 1.0
}

/// Size first, then position. If the app refuses to shrink, hold the edge that was
/// supposed to stay put, so it never slides out of its corner of the group.
fn place(el: &Elem, now: Rect, to: Rect) -> Option<Rect> {
    let mut got = now;
    if (now.w - to.w).abs() > 0.5 || (now.h - to.h).abs() > 0.5 {
        el.set_size(to.w, to.h);
        got = el.frame()?;
    }
    let held = |start: f64, size: f64, want: f64, to_start: f64| {
        let end_stays = (start - to_start).abs() > 0.5 && size > want + 2.0;
        if end_stays && to_start > start { to_start + want - size } else { to_start }
    };
    let x = held(now.x, got.w, to.w, to.x);
    let y = held(now.y, got.h, to.h, to.y);
    if (got.x - x).abs() > 0.5 || (got.y - y).abs() > 0.5 {
        el.set_position(x, y);
        got = el.frame()?;
    }
    Some(got)
}

fn run(el: &Elem, wid: u32, rx: &Receiver<Rect>) {
    // (asked, got, since when) of a refusal per axis, to tell a real minimum from an app that
    // is slow or busy for a moment
    let mut refused: [Option<(f64, f64, Instant)>; 2] = [None, None];
    while let Ok(mut target) = rx.recv() {
        while let Ok(newer) = rx.try_recv() {
            target = newer;
        }
        let mut tries = 0;
        let Some(mut now) = el.frame() else { continue };
        while !close(&now, &target) && tries < 14 {
            let Some(got) = place(el, now, target) else { break };
            now = got;
            let newest = rx.try_iter().last();
            for (axis, asked, got) in [(0, target.w, now.w), (1, target.h, now.h)] {
                if got > asked + 2.0 {
                    // same size for two different requests, or still refusing when idle
                    let before = refused[axis].filter(|r| r.1 == got);
                    let lasted = before.map_or(Duration::ZERO, |r| r.2.elapsed());
                    let asked_again = before.is_some_and(|r| r.0 != asked);
                    // a busy app keeps its old size for a moment; a real minimum stays
                    let sure = newest.is_none() && tries >= 9 && lasted > Duration::from_millis(500);
                    if sure || (asked_again && lasted > Duration::from_millis(90)) {
                        MINIMUMS.lock().unwrap().push(Minimum { wid, width: axis == 0, size: got.ceil(), sure });
                    }
                    refused[axis] = Some((asked, got, before.map_or_else(Instant::now, |r| r.2)));
                } else {
                    refused[axis] = None;
                }
            }
            match newest {
                Some(r) => {
                    target = r;
                    tries = 0;
                }
                None => {
                    tries += 1;
                    if !close(&now, &target) {
                        // quick at first, then patient
                        std::thread::sleep(Duration::from_millis(if tries < 4 { 12 } else { 70 }));
                        now = el.frame().unwrap_or(now);
                    }
                }
            }
        }
    }
}
