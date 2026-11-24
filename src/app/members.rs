//! Windows joining and leaving groups.

use super::commands::Outcome;
use super::group::{Group, Member};
use super::{App, GroupId, MAX_TILES, try_with};
use crate::core::geometry::Rect;
use crate::core::layout::Layout;
use crate::core::window::{self, WindowRef};
use crate::platform::element::{Change, Element, Observer};
use crate::platform::{apps, windows};
use std::collections::hash_map::Entry;

impl App {
    /// Hear about a window being closed or minimised.
    pub(super) fn watch_window(&mut self, m: &Member) {
        if let Entry::Vacant(e) = self.observers.entry(m.pid) {
            let Some(o) = Observer::new(m.pid, window_changed) else { return };
            e.insert(o);
        }
        if let Some(o) = self.observers.get(&m.pid) {
            o.watch(&m.element);
        }
    }

    /// Where a window is: group and member, by position.
    pub(super) fn find_member(&self, is: impl Fn(&Member) -> bool) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(gi, g)| Some((gi, g.members.iter().position(&is)?)))
    }

    /// Minimised: the neighbours take the space. Back: it gets its place again.
    fn set_minimized(&mut self, gi: usize, mi: usize, minimized: bool) {
        let g = &mut self.groups[gi];
        if g.layout.is_hidden(mi) == minimized {
            return;
        }
        self.seam_drag = None;
        g.layout.set_hidden(mi, minimized);
        let wid = g.members[mi].wid;
        g.place_smoothly(&windows::ordinary(), Some(wid));
    }

    /// Take a window out of its group; the neighbour gets its space.
    pub(super) fn drop_member(&mut self, gi: usize, mi: usize) {
        self.seam_drag = None;
        let g = &mut self.groups[gi];
        let wid = g.members.remove(mi).wid;
        g.stack.retain(|s| s.wid != wid);
        if g.members.len() < 2 {
            self.groups.remove(gi);
            self.restore_dock_if_idle();
        } else {
            g.layout.remove(mi);
            g.update_name();
            g.place_smoothly(&windows::ordinary(), None);
        }
    }

    /// Make a group of these windows, keeping their arrangement.
    pub(super) fn make_group(&mut self, picks: &[WindowRef], region: Rect) -> Outcome {
        let order = windows::ordinary();
        let found: Vec<(Rect, Member)> =
            picks.iter().filter_map(|w| Some((window::find(&order, w.wid)?.bounds, Member::open(*w)?))).collect();
        if found.len() < 2 {
            return Err("Need at least two windows that are on screen".into());
        }
        self.seam_drag = None;
        // a window is only ever in one group
        self.groups.retain(|g| !found.iter().any(|(_, m)| g.has(m.wid)));
        self.restore_dock_if_idle();

        let (rects, members): (Vec<Rect>, Vec<Member>) = found.into_iter().unzip();
        for m in &members {
            self.watch_window(m);
            self.groups.iter_mut().for_each(|g| g.forget(m.wid));
        }
        self.ids_given += 1;
        let mut g = Group::new(self.ids_given, members, Layout::from_rects(region, &rects));
        // the windowed size must not be measured while the Dock is hidden
        if let Some(windowed) = self.groups.iter().find(|o| o.full).map(|o| o.windowed) {
            if let Some(r) = windowed {
                g.layout.set_region(r);
            }
            g.set_full(true);
        }
        g.used = self.last_focused.map_or(0, |w| w.wid);
        g.place_smoothly(&order, None);
        let n = g.members.len();
        self.active = Some(g.id);
        self.groups.push(g);
        Ok(format!("Tiled {n} windows"))
    }

    /// Add windows to a group, splitting the tile last used.
    pub(super) fn add_to(&mut self, id: GroupId, picks: &[WindowRef]) -> Outcome {
        let order = windows::ordinary();
        let Some(g) = self.group(id) else { return Err("No group".into()) };
        // tiles to split by preference: the one in use, and what we add to it
        let mut near = vec![g.members[g.index_of(g.used).unwrap_or(0)].wid];
        let mut added = 0;
        let mut refused = None;
        for &w in picks {
            let Some(g) = self.group(id) else { return Err("No group".into()) };
            if g.has(w.wid) {
                continue;
            }
            if g.members.len() >= MAX_TILES {
                refused = Some(format!("No more room: a group holds {MAX_TILES} windows"));
                break;
            }
            let Some(m) = Member::open(w) else { continue };
            let Some(layout) = with_one_more(g, &near) else {
                refused = Some(format!("No room for {}: the tiles would get too small", apps::name(w.pid)));
                continue;
            };
            // a window is only ever in one group
            if let Some((gi, mi)) = self.find_member(|m| m.wid == w.wid) {
                self.drop_member(gi, mi);
            }
            self.groups.iter_mut().for_each(|g| g.forget(w.wid));
            self.watch_window(&m);
            let Some(g) = self.group_mut(id) else { return Err("No group".into()) };
            g.layout = layout;
            g.members.push(m);
            near.push(w.wid);
            added += 1;
        }
        if added > 0 {
            self.seam_drag = None;
            if let Some(g) = self.group_mut(id) {
                g.update_name();
                g.place_smoothly(&order, None);
            }
        }
        refused.map_or(Ok(format!("Added {added}")), Err)
    }
}

/// The layout with one tile split: the largest of `near`; failing that, the largest of all.
fn with_one_more(g: &Group, near: &[u32]) -> Option<Layout> {
    let frames = g.layout.frames();
    let largest =
        |tiles: &mut dyn Iterator<Item = usize>| tiles.max_by(|a, b| frames[*a].area().total_cmp(&frames[*b].area()));
    let choices = [largest(&mut near.iter().filter_map(|w| g.index_of(*w))), largest(&mut (0..g.members.len()))];
    choices.into_iter().flatten().find_map(|i| {
        let mut layout = g.layout.clone();
        layout.split(i);
        layout.fits().then_some(layout)
    })
}

fn window_changed(change: Change, window: &Element) {
    try_with(|a| {
        let Some((gi, mi)) = a.find_member(|m| m.element.same(window)) else { return };
        match change {
            Change::Closed => a.drop_member(gi, mi),
            Change::Minimized => a.set_minimized(gi, mi, true),
            Change::Restored => a.set_minimized(gi, mi, false),
        }
    });
}
