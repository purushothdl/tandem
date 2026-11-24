//! Keeping a group's windows together, front to back.
//!
//! macOS keeps all windows of the active app above other apps' raised windows, and a raise
//! lands just below the active app's. Most of this file follows from that; see docs/QUIRKS.md.

use super::group::{Group, Member};
use crate::core::window::{self, Window, WindowRef};
use crate::platform::element::{Element, find_window};
use crate::platform::focus;
use std::time::{Duration, Instant};

/// Let a raise take effect before asking again.
const RAISE_AGAIN: Duration = Duration::from_millis(50);
const HAND_OVER_AGAIN: Duration = Duration::from_millis(45);
/// Windows overlapping the group by less than this are not in its way.
const OVERLAP: f64 = 40.0;

impl Group {
    /// What should sit under `focus`, front to back, as last seen.
    fn wanted(&self, focus: u32, order: &[Window]) -> Vec<WindowRef> {
        let mut v = self.stack.clone();
        let missing = self.members.iter().filter(|m| !self.stack.iter().any(|s| s.wid == m.wid));
        v.extend(missing.map(Member::reference));
        v.retain(|s| s.wid != focus && window::find(order, s.wid).is_some());
        v
    }

    /// Is the group together, with only its own panes in between? In Mission Control the
    /// list holds `thumbnails`, whose positions mean nothing.
    pub(super) fn in_place(&self, focus: u32, order: &[Window], thumbnails: bool) -> bool {
        let want: Vec<u32> = std::iter::once(focus).chain(self.wanted(focus, order).iter().map(|s| s.wid)).collect();
        let Some(rear) = order.iter().rposition(|w| want.contains(&w.id)) else { return true };
        let r = self.layout.region();
        let overlaps = |w: &Window| {
            w.bounds.right().min(r.right()) - w.bounds.x.max(r.x) > OVERLAP
                && w.bounds.bottom().min(r.bottom()) - w.bounds.y.max(r.y) > OVERLAP
        };
        if order[..rear].iter().any(|w| !want.contains(&w.id) && (thumbnails || overlaps(w))) {
            return false;
        }
        // how tiles sit among themselves does not matter, unless panes are between them
        let has_panes = want.iter().any(|w| !self.has(*w));
        !has_panes || order.iter().filter(|w| want.contains(&w.id)).map(|w| w.id).eq(want.iter().copied())
    }

    /// Bring the rest of the group up under `focus`, which is already on top. False if it was
    /// too soon to try again.
    pub(super) fn raise_rest(&mut self, focus: u32, order: &[Window]) -> bool {
        if within(self.raised_at, RAISE_AGAIN) {
            return false;
        }
        let want = self.wanted(focus, order);
        if self.sibling_in_between(focus, &want, order) {
            return self.hand_over(focus, &want);
        }
        self.raised_at = Some(Instant::now());
        let has_panes = want.iter().any(|w| !self.has(w.wid));
        let list: Vec<(Option<Element>, WindowRef)> = (want.into_iter().rev())
            .map(|w| (self.members.iter().find(|m| m.wid == w.wid).map(|m| m.element.clone()), w))
            .collect();
        // each raise lands just below the active window, so rear to front rebuilds the stack
        let raise = |(known, w): (Option<Element>, WindowRef)| {
            if let Some(e) = known.or_else(|| find_window(w.pid, w.wid)) {
                raise_if_shown(&e);
            }
        };
        if has_panes {
            std::thread::spawn(move || list.into_iter().for_each(raise));
        } else {
            // no order to keep: all at once, so a slow app delays only itself
            for w in list {
                std::thread::spawn(move || raise(w));
            }
        }
        true
    }

    /// Another window of the focused app sits inside the group's stack, and none of that
    /// app's other windows belong to the group. Raising cannot get past such a window.
    fn sibling_in_between(&self, focus: u32, want: &[WindowRef], order: &[Window]) -> bool {
        let Some(pid) = window::find(order, focus).map(|w| w.pid) else { return false };
        let wanted = |wid: u32| want.iter().any(|s| s.wid == wid);
        let rear = order.iter().rposition(|w| wanted(w.id)).unwrap_or(0);
        order[..rear].iter().any(|w| w.pid == pid && w.id != focus && !wanted(w.id) && !self.has(w.id))
            && want.iter().all(|s| s.pid != pid)
    }

    /// Activate each app of the group in turn, then go back to the chosen window.
    fn hand_over(&mut self, focus: u32, want: &[WindowRef]) -> bool {
        if within(self.handed_over_at, HAND_OVER_AGAIN) {
            return false;
        }
        let member = |wid: u32| self.members.iter().find(|m| m.wid == wid).map(|m| (m.element.clone(), m.reference()));
        let Some((chosen, at)) = member(focus) else { return false };
        let others: Vec<_> = want.iter().rev().filter_map(|w| member(w.wid)).collect();
        self.handed_over_at = Some(Instant::now());
        std::thread::spawn(move || {
            for (e, w) in others {
                if !e.is_minimized() {
                    focus::activate(w.pid, &e);
                }
            }
            // first the app without its windows, then just this one
            if !focus::focus_without_raising(at.pid, at.wid) {
                focus::activate(at.pid, &chosen);
            }
            chosen.raise();
        });
        true
    }

    /// Remember the stacking while the group is in use.
    pub(super) fn remember(&mut self, order: &[Window]) {
        let Some(rear) = order.iter().rposition(|w| self.has(w.id)) else { return };
        let region = self.layout.region();
        self.panes.retain(|p| window::find(order, *p).is_some_and(|w| window::is_small(&w.bounds, &region)));
        self.stack = (order[..=rear].iter())
            .filter(|w| self.has(w.id) || self.panes.contains(&w.id))
            .map(Window::reference)
            .collect();
    }

    /// Raise every member that is not of this app.
    pub(super) fn raise_except(&self, pid: i32) {
        for m in self.members.iter().filter(|m| m.pid != pid) {
            let e = m.element.clone();
            std::thread::spawn(move || raise_if_shown(&e));
        }
    }
}

fn raise_if_shown(e: &Element) {
    if !e.is_minimized() {
        e.raise();
    }
}

fn within(t: Option<Instant>, d: Duration) -> bool {
    t.is_some_and(|t| t.elapsed() < d)
}
