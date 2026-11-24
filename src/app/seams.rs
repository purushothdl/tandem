//! Dragging the seam between tiles.

use super::{App, DIALOG_OPEN, GroupId, own_pid, writer};
use crate::core::geometry::Axis;
use crate::core::layout::Seam;
use crate::platform::log::log;
use crate::platform::mouse::Verdict;
use crate::platform::windows;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// How close to a seam a press must be to grab it.
const GRAB: f64 = 6.0;

#[derive(Clone, Copy)]
pub(super) struct SeamDrag {
    pub group: GroupId,
    pub seam: usize,
    /// From the seam to the pointer.
    offset: f64,
}

fn across(s: &Seam, x: f64, y: f64) -> f64 {
    if s.axis == Axis::X { x } else { y }
}

fn along(s: &Seam, x: f64, y: f64) -> f64 {
    across(s, y, x)
}

impl App {
    /// The seam under a point, if nothing else is in front of it there.
    pub(super) fn seam_at(&self, x: f64, y: f64) -> Option<(GroupId, usize)> {
        if DIALOG_OPEN.load(Ordering::Relaxed) {
            return None;
        }
        let near = |s: &Seam| (across(s, x, y) - s.at).abs() <= GRAB && (s.from..s.to).contains(&along(s, x, y));
        let hits: Vec<(GroupId, usize)> = (self.groups.iter())
            // only the group in use, even where another one shows around its edges
            .filter(|g| self.active.is_none_or(|id| id == g.id))
            .filter_map(|g| Some((g.id, g.layout.seams().iter().position(near)?)))
            .collect();
        if hits.is_empty() {
            return None;
        }
        let all = windows::all();
        if windows::mission_control_open(&all) {
            return None;
        }
        let here = || all.iter().filter(|w| w.pid != own_pid() && w.bounds.contains(x, y));
        // an open menu or floating panel above the seam owns the press
        if here().any(|w| w.is_menu_or_panel()) {
            return None;
        }
        // several groups can have a seam at the same place; the one on top wins
        let top = here().find(|w| w.is_visible_normal())?;
        hits.into_iter().find(|(id, _)| self.group(*id).is_some_and(|g| g.has(top.id)))
    }

    /// A press on a seam starts dragging it; a double-click evens the sizes first.
    pub(super) fn grab_seam(&mut self, (id, k): (GroupId, usize), x: f64, y: f64, clicks: i64) -> Verdict {
        let Some(g) = self.group_mut(id) else { return Verdict::Pass };
        if clicks == 2 {
            g.layout.equalize();
            g.place_smoothly(&windows::ordinary(), None);
        }
        let Some(s) = g.layout.seams().get(k).copied() else { return Verdict::Pass };
        g.mins_before_drag = Some(g.layout.mins().to_vec());
        self.seam_drag = Some(SeamDrag { group: id, seam: k, offset: across(&s, x, y) - s.at });
        Verdict::Swallow
    }

    pub(super) fn drag_seam(&mut self, x: f64, y: f64) {
        self.learn_minimums();
        let Some(drag) = self.seam_drag else { return };
        let Some(g) = self.group_mut(drag.group) else { return };
        let Some(before) = g.layout.seams().get(drag.seam).copied() else { return };
        g.layout.drag_seam(drag.seam, across(&before, x, y) - drag.offset);
        if g.layout.seams().get(drag.seam).map(|s| s.at) != Some(before.at) {
            g.seam_moved_at = Some(Instant::now());
            g.place();
        }
    }

    /// Minimum sizes the writers found out. Applied even mid-drag, so the seam stops dead.
    pub(super) fn learn_minimums(&mut self) {
        let dragging = self.seam_drag.map(|d| d.group);
        for (wid, axis, min) in writer::take_minimums() {
            for g in &mut self.groups {
                let Some(i) = g.index_of(wid) else { continue };
                if min.sure {
                    // also what to go back to after the drag
                    if let Some(known) = g.mins_before_drag.as_mut().and_then(|v| v.get_mut(i)) {
                        let known = crate::core::layout::on_axis(known, axis);
                        *known = known.max(min.size);
                    }
                } else if dragging != Some(g.id) {
                    continue;
                }
                if g.layout.learn_min(i, axis, min.size) {
                    let only = if min.sure { "" } else { " (for this drag)" };
                    log(&format!("window {wid}: minimum {axis:?} {}{only}", min.size));
                    if !g.layout.fits() {
                        log("these windows' minimum sizes do not fit together");
                    }
                    g.place();
                }
            }
        }
        if dragging.is_none() {
            for g in &mut self.groups {
                if let Some(m) = g.mins_before_drag.take() {
                    g.layout.restore_mins(&m);
                }
            }
        }
    }
}
