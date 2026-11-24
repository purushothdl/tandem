//! Mouse presses, and dragging windows by their title bars: a tile onto a tile to swap them,
//! or with Shift a window from outside onto half of a tile.

use super::group::Member;
use super::{App, DIALOG_OPEN, GroupId, MAX_TILES, own_pid};
use crate::core::geometry::{Axis, Rect};
use crate::core::gesture::{half_at, is_title_drag};
use crate::core::window::{self, WindowRef};
use crate::platform::log::log;
use crate::platform::mouse::{Event, Kind, Verdict};
use crate::platform::windows;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// The pointer must move this far before we look whether the window follows.
const DRAG_FROM: f64 = 12.0;
const LOOK_EVERY: Duration = Duration::from_millis(40);
/// An app refuses to be resized until it has seen the button go up.
const AFTER_RELEASE: Duration = Duration::from_millis(110);

/// A press on a window that may become a drag by its title bar.
pub(super) struct Press {
    pub group: GroupId,
    window: WindowRef,
    start: (f64, f64),
    from: Rect,
    /// The window is really being dragged (not text selected in it).
    pub moving: bool,
    looked: Instant,
    pub target: Target,
}

/// What dropping the window would do.
pub(super) enum Target {
    /// A tile: trade places with another one.
    Swap { with: Option<u32> },
    /// A window of no group: with Shift, share a tile.
    Join(Option<Half>),
}

#[derive(Clone, Copy)]
pub(super) struct Half {
    tile: u32,
    axis: Axis,
    /// Left or above, rather than right or below.
    before: bool,
    pub rect: Rect,
}

/// A window dropped on half of a tile, joining shortly.
pub(super) struct Joining {
    due: Instant,
    group: GroupId,
    window: WindowRef,
    home: Rect,
    half: Half,
}

impl App {
    pub(super) fn mouse(&mut self, e: Event) -> Verdict {
        match e.kind {
            Kind::Down => self.mouse_down(e),
            Kind::Dragged if self.press.is_some() => {
                self.drag_window(e);
                Verdict::Pass
            }
            Kind::Up if self.press.is_some() => {
                self.drop_window(e);
                Verdict::Pass
            }
            Kind::Dragged if self.seam_drag.is_some() => {
                self.drag_seam(e.x, e.y);
                Verdict::Swallow
            }
            Kind::Up if self.seam_drag.is_some() => {
                self.drag_seam(e.x, e.y);
                self.seam_drag = None;
                Verdict::Swallow
            }
            _ => Verdict::Pass,
        }
    }

    fn mouse_down(&mut self, e: Event) -> Verdict {
        self.seam_drag = None;
        if self.groups.is_empty() || on_own_dialog(e.x, e.y) {
            return Verdict::Pass;
        }
        if let Some(hit) = self.seam_at(e.x, e.y) {
            return self.grab_seam(hit, e.x, e.y, e.clicks);
        }
        self.switch = None;
        self.press = None;
        let all = windows::all();
        if windows::mission_control_open(&all) {
            return Verdict::Pass; // watch_mission_control sees what gets picked
        }
        let order = window::ordinary(&all);
        let Some(top) = window::on_top_at(&order, e.x, e.y) else { return Verdict::Pass };
        let id = self.group_of(top.id).map(|g| g.id);
        // inside the group in use, a click is macOS's business alone
        if id.is_some() && id != self.active {
            self.come_to(id, top.id);
        }
        let target = if id.is_some() { Target::Swap { with: None } } else { Target::Join(None) };
        self.press = id.or(self.active).map(|group| Press {
            group,
            window: top.reference(),
            start: (e.x, e.y),
            from: top.bounds,
            moving: false,
            looked: Instant::now(),
            target,
        });
        Verdict::Pass
    }

    fn drag_window(&mut self, e: Event) {
        let Some(p) = &mut self.press else { return };
        if !p.moving {
            // only now and then: this runs for every drag anywhere in a tile
            let moved = (e.x - p.start.0).hypot(e.y - p.start.1);
            if moved < DRAG_FROM || p.looked.elapsed() < LOOK_EVERY {
                return;
            }
            p.looked = Instant::now();
            let order = windows::ordinary();
            p.moving = window::find(&order, p.window.wid).is_some_and(|w| is_title_drag(&p.from, &w.bounds));
            if !p.moving {
                return;
            }
        }
        let g = self.groups.iter().find(|g| g.id == p.group);
        let hit = g.and_then(|g| Some((g, g.tile_at(e.x, e.y)?)));
        match &mut p.target {
            Target::Swap { with } => {
                *with = hit.map(|(g, i)| g.members[i].wid).filter(|wid| *wid != p.window.wid);
            }
            Target::Join(half) => {
                *half = hit.filter(|_| e.shift).and_then(|(g, i)| {
                    let (axis, before) = half_at(&g.layout.frames()[i], e.x, e.y);
                    let mut layout = g.layout.clone();
                    let new = layout.insert(i, axis, before);
                    let rect = layout.frames()[new];
                    layout.fits().then(|| Half { tile: g.members[i].wid, axis, before, rect })
                });
            }
        }
    }

    fn drop_window(&mut self, e: Event) {
        self.drag_window(e);
        let Some(p) = self.press.take().filter(|p| p.moving) else { return };
        let due = Instant::now() + AFTER_RELEASE;
        match p.target {
            Target::Join(half) => {
                let joining = |half| Joining { due, group: p.group, window: p.window, home: p.from, half };
                self.joining = half.map(joining).or(self.joining.take());
            }
            Target::Swap { with } => {
                let Some(g) = self.group_mut(p.group) else { return };
                if let (Some(i), Some(j)) = (g.index_of(p.window.wid), with.and_then(|w| g.index_of(w))) {
                    if g.layout.swap(i, j) {
                        g.members.swap(i, j);
                        g.update_name();
                    } else {
                        log("no swap: one of them does not fit in the other's place");
                    }
                }
                g.changed_at = Instant::now();
                g.settle_at = Some(due);
            }
        }
    }

    /// Let a dropped window join, once its app has seen the release.
    pub(super) fn finish_joining(&mut self) {
        let Some(j) = self.joining.take_if(|j| j.due <= Instant::now()) else { return };
        let Some(g) = self.group(j.group) else { return };
        let Some(i) = g.index_of(j.half.tile) else { return };
        if g.members.len() >= MAX_TILES {
            log("no join: the group is full");
            return;
        }
        let mut layout = g.layout.clone();
        layout.insert(i, j.half.axis, j.half.before);
        if !layout.fits() {
            log("no join: it does not fit there");
            return;
        }
        let Some(mut m) = Member::open(j.window) else { return };
        m.home = Some(j.home);
        self.watch_window(&m);
        let wid = j.window.wid;
        self.groups.iter_mut().for_each(|g| g.forget(wid));
        let Some(g) = self.group_mut(j.group) else { return };
        g.layout = layout;
        g.members.push(m);
        g.used = wid;
        g.update_name();
        g.place_smoothly(&windows::ordinary(), None);
        self.come_to(Some(j.group), wid);
    }
}

/// A press on one of our dialogs is not a press on the tile behind it. (Our bars and
/// outlines are too thin or let presses through.)
fn on_own_dialog(x: f64, y: f64) -> bool {
    DIALOG_OPEN.load(Ordering::Relaxed)
        && windows::all()
            .iter()
            .any(|w| w.pid == own_pid() && w.bounds.h > 60.0 && w.bounds.w > 60.0 && w.bounds.contains(x, y))
}
