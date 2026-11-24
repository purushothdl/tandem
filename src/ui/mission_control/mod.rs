//! What we add to Mission Control: the bar of groups, an outline around the partners of the
//! window pointed at, and a pill with the group's name where there is no room for the bar.

mod bar;
mod mark;

use crate::app::views;
use mark::Mark;
use objc2::MainThreadMarker;
use objc2_quartz_core::CATransaction;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Thumbnails must have been still this long before marks show. A slow swipe keeps them
/// moving, with short pauses; showing in each pause flickered.
const STILL: Duration = Duration::from_millis(220);
/// Moved within the last frame.
const MOVING: Duration = Duration::from_millis(20);

thread_local! {
    /// By window. Kept between openings: building them is slow enough to see.
    static MARKS: RefCell<HashMap<u32, Mark>> = RefCell::default();
    static MOVED: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// A press usually closes Mission Control: take everything off the screen right away, before
/// the calls to other apps that bringing up a group takes.
pub fn drop_on_press() {
    MARKS.with(|m| {
        let mut marks = m.borrow_mut();
        if marks.values().any(Mark::is_outlined) && bar::pressed() {
            marks.values_mut().for_each(Mark::hide);
            CATransaction::flush();
        }
    });
}

/// Every frame, with the pointer here.
pub fn refresh(mtm: MainThreadMarker, x: f64, y: f64) {
    let Some(thumbnails) = views::thumbnails(x, y) else { return };
    MARKS.with(|m| {
        let marks = &mut *m.borrow_mut();
        if thumbnails.is_empty() {
            marks.values_mut().for_each(Mark::hide);
            bar::hide();
            MOVED.set(None);
            return;
        }
        marks.retain(|wid, mark| {
            let keep = thumbnails.iter().any(|t| t.wid == *wid && t.name == mark.name);
            if !keep {
                mark.hide();
            }
            keep
        });
        let mut moved = MOVED.get().unwrap_or_else(Instant::now);
        for t in &thumbnails {
            if marks.entry(t.wid).or_insert_with(|| Mark::new(mtm, t)).move_to(t.at) {
                moved = Instant::now();
            }
        }
        MOVED.set(Some(moved));
        let calm = moved.elapsed();
        if calm < MOVING {
            // no fade: an outline left behind while its window grows looks stretched
            marks.values_mut().for_each(Mark::hide);
            CATransaction::flush();
        }

        let bar = bar::refresh(mtm, calm, x, y);
        if bar.away {
            marks.values_mut().for_each(Mark::hide);
            return;
        }
        let still = calm > STILL;
        let hovered = thumbnails.iter().find(|t| t.hovered);
        for t in &thumbnails {
            // macOS outlines the one under the pointer itself; its partners are ours
            let partner = hovered.is_some_and(|h| h.group == t.group && h.wid != t.wid);
            if let Some(mark) = marks.get_mut(&t.wid) {
                mark.update(still && !bar.showing, still && (partner || bar.hot == Some(t.group)));
            }
        }
    });
}
