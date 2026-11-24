//! Window groups: what is in them, where the seams are, and the only code allowed to change that.
//!
//! The layout is the truth. It changes when the user drags a seam (which this app
//! handles itself), uses the menu, or an app reports a minimum width. A window found
//! anywhere else is put back; it never reshapes the group.

use crate::ax::{self, Elem, Observer, RawElem, WinInfo};
use crate::chrome;
use crate::layout::{Layout, Rect, Seam};
use crate::writer;
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
use core_foundation_sys::runloop::{
    kCFRunLoopCommonModes, CFRunLoopAddTimer, CFRunLoopGetMain, CFRunLoopTimerCreate, CFRunLoopTimerRef,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSRunningApplication, NSScreen, NSWorkspace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// How close to a seam a press must be to grab it.
const GRAB: f64 = 6.0;
pub const MAX_TILES: usize = 6;
/// How long windows take to glide into place.
const GLIDE: f64 = 0.24;

struct Member {
    elem: Elem,
    pid: i32,
    wid: u32,
    tx: Sender<Rect>,
    /// A frame the app keeps coming back to; asking again would be pointless.
    stuck_at: Option<Rect>,
}

impl Member {
    fn open(pid: i32, wid: u32) -> Option<Member> {
        let elem = ax::find_window(pid, wid)?;
        elem.set_timeout(0.4);
        let tx = writer::spawn(elem.clone(), wid);
        Some(Member { elem, pid, wid, tx, stuck_at: None })
    }
}

struct Group {
    id: u32,
    /// Front to back, as last seen in use: the tiles, and the small windows ("panes") in
    /// front of any of them. What to restore when the user comes back. (pid, window id)
    stack: Vec<(i32, u32)>,
    /// Windows the user opened or clicked over this group; nothing else counts as a pane.
    panes: Vec<u32>,
    /// The tile last used, to return the keyboard to.
    used: u32,
    raised_at: Option<Instant>,
    members: Vec<Member>,
    layout: Layout,
    expected: Vec<Rect>,
    changed_at: Instant,
    /// Last time the user actually moved a seam.
    seam_moved_at: Option<Instant>,
    /// Windows gliding to `expected`: where each started, and when.
    glide: Option<(Vec<Rect>, Instant)>,
    name: String,
    full: bool,
    windowed: Option<Rect>,
}

struct App {
    /// The focused window of the frontmost other app: (pid, window id).
    last: Option<(i32, u32)>,
    groups: Vec<Group>,
    observers: HashMap<i32, Observer>,
    saved: Option<chrome::Saved>,
    /// Seam being dragged: (group, seam, pointer offset from the seam).
    drag: Option<(usize, usize, f64)>,
    counter: u32,
    /// A raise to double-check: (focused window, when it started, retries used).
    verify: Option<(u32, Instant, usize)>,
    /// Mission Control was open until at least this moment.
    mc_seen: Option<Instant>,
    /// Our window whose Mission Control thumbnail is being clicked.
    mc_pick: Option<u32>,
    front_pid: i32,
    next_id: u32,
    /// The group being worked in (also while using a window floating over it).
    active: Option<u32>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App {
        last: None,
        groups: Vec::new(),
        observers: HashMap::new(),
        saved: None,
        drag: None,
        counter: 0,
        verify: None,
        mc_seen: None,
        mc_pick: None,
        front_pid: 0,
        next_id: 1,
        active: None,
    });
}

fn with<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

pub fn log(msg: &str) {
    use std::io::Write;
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/Library/Logs/wingroup.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

fn primary_visible() -> Rect {
    let Some(mtm) = MainThreadMarker::new() else { return ax::main_display_bounds() };
    let screens = NSScreen::screens(mtm);
    if screens.count() == 0 {
        return ax::main_display_bounds();
    }
    let s = screens.objectAtIndex(0);
    let (f, v) = (s.frame(), s.visibleFrame());
    Rect { x: v.origin.x, y: f.size.height - (v.origin.y + v.size.height), w: v.size.width, h: v.size.height }
}

fn default_name(g: &Group) -> String {
    let mut names: Vec<String> = Vec::new();
    for m in &g.members {
        let n = app_name(m.pid);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names.join(" + ")
}

/// Small enough to float over a group rather than replace it.
fn is_small(w: &Rect, region: &Rect) -> bool {
    w.w * w.h < 0.6 * region.w * region.h
}

fn on_top_at(order: &[WinInfo], x: f64, y: f64) -> Option<&WinInfo> {
    order.iter().find(|w| w.bounds.contains(x, y))
}

/// Share of `order[i]` not hidden behind the windows in front of it.
fn visible_share(order: &[WinInfo], i: usize) -> f64 {
    let b = order[i].bounds;
    let n = 12;
    let mut seen = 0;
    for a in 0..n {
        for c in 0..n {
            let x = b.x + b.w * (a as f64 + 0.5) / n as f64;
            let y = b.y + b.h * (c as f64 + 0.5) / n as f64;
            if !order[..i].iter().any(|w| w.bounds.contains(x, y)) {
                seen += 1;
            }
        }
    }
    seen as f64 / (n * n) as f64
}

impl Group {
    /// Put the windows where the layout says, at once.
    fn place(&mut self) {
        self.glide = None;
        let frames = self.layout.frames();
        for (j, (m, f)) in self.members.iter_mut().zip(&frames).enumerate() {
            m.stuck_at = None;
            if self.expected.get(j) != Some(f) {
                let _ = m.tx.send(*f);
            }
        }
        self.expected = frames;
        self.changed_at = Instant::now();
    }

    /// The same, but let the windows glide there from where they are now.
    fn place_smoothly(&mut self, order: &[WinInfo]) {
        let to = self.layout.frames();
        let from: Vec<Rect> = self
            .members
            .iter()
            .zip(&to)
            .map(|(m, t)| order.iter().find(|w| w.id == m.wid).map_or(*t, |w| w.bounds))
            .collect();
        let far = from.iter().zip(&to).map(|(a, b)| a.distance(b)).fold(0.0, f64::max);
        let calm = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
        self.expected.clear(); // ask every window again, wherever it is
        self.place();
        if far > 24.0 && !calm {
            self.glide = Some((from, Instant::now()));
            self.step();
        }
    }

    /// One frame of the glide: fast start, soft landing.
    fn step(&mut self) {
        let Some((from, t0)) = &self.glide else { return };
        let t = (t0.elapsed().as_secs_f64() / GLIDE).min(1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        for ((m, a), b) in self.members.iter().zip(from).zip(&self.expected) {
            let _ = m.tx.send(a.lerp(b, e));
        }
        self.changed_at = Instant::now();
        if t >= 1.0 {
            self.glide = None;
        }
    }

    fn has(&self, wid: u32) -> Option<usize> {
        self.members.iter().position(|m| m.wid == wid)
    }

    /// Put back any window that is not where the layout says.
    fn correct(&mut self, order: &[WinInfo]) {
        if self.glide.is_some() || self.changed_at.elapsed() < Duration::from_millis(400) {
            return;
        }
        let mut off = false;
        for (m, exp) in self.members.iter_mut().zip(&self.expected) {
            let Some(w) = order.iter().find(|w| w.id == m.wid) else { continue };
            if w.bounds.near(exp) {
                m.stuck_at = None;
            } else if m.stuck_at != Some(w.bounds) {
                off = true;
            }
        }
        if off {
            let seen: Vec<Option<Rect>> = self.members.iter().map(|m| order.iter().find(|w| w.id == m.wid).map(|w| w.bounds)).collect();
            self.place_smoothly(order);
            for ((m, s), exp) in self.members.iter_mut().zip(seen).zip(&self.expected) {
                m.stuck_at = s.filter(|b| !b.near(exp));
            }
        }
    }

    /// The stacking wanted while `focus` is on top: as last seen, with `focus` in front.
    fn wanted(&self, focus: u32, order: &[WinInfo]) -> Vec<(i32, u32)> {
        let mut v: Vec<(i32, u32)> = self.stack.clone();
        v.extend(self.members.iter().filter(|m| !self.stack.iter().any(|s| s.1 == m.wid)).map(|m| (m.pid, m.wid)));
        v.retain(|s| s.1 != focus && order.iter().any(|w| w.id == s.1));
        v
    }

    /// Is the group together, with only its own panes in between?
    fn in_place(&self, focus: u32, order: &[WinInfo], thumbnails: bool) -> bool {
        let want: Vec<u32> = std::iter::once(focus).chain(self.wanted(focus, order).iter().map(|s| s.1)).collect();
        let Some(rear) = order.iter().rposition(|w| want.contains(&w.id)) else { return true };
        let r = self.layout.region();
        let foreign = order[..rear].iter().any(|w| {
            !want.contains(&w.id)
                // thumbnails in Mission Control: positions mean nothing, only the order
                && (thumbnails
                    || ((w.bounds.right().min(r.right()) - w.bounds.x.max(r.x)) > 40.0
                        && (w.bounds.bottom().min(r.bottom()) - w.bounds.y.max(r.y)) > 40.0))
        });
        if foreign {
            return false;
        }
        // how tiles sit among themselves does not matter, unless panes are between them
        let has_panes = want.iter().any(|w| self.has(*w).is_none());
        !has_panes || order.iter().filter(|w| want.contains(&w.id)).map(|w| w.id).eq(want.iter().copied())
    }

    /// Bring the rest of the group up under `focus`, which is already on top. Each raise
    /// lands just below the active window, so going rear to front rebuilds the stacking.
    fn raise_rest(&mut self, focus: u32, order: &[WinInfo]) {
        if self.raised_at.is_some_and(|t| t.elapsed() < Duration::from_millis(50)) {
            return;
        }
        self.raised_at = Some(Instant::now());
        let want = self.wanted(focus, order);
        let panes = want.iter().any(|w| self.has(w.1).is_none());
        let elem = |wid: u32| self.members.iter().find(|m| m.wid == wid).map(|m| writer::SendElem(m.elem.clone()));
        let up = |e: Option<writer::SendElem>, pid: i32, wid: u32| {
            if let Some(e) = e.map(|e| e.0).or_else(|| ax::find_window(pid, wid)) {
                if !e.is_minimized() {
                    e.raise();
                }
            }
        };
        let list: Vec<_> = want.iter().rev().map(|&(pid, wid)| (elem(wid), pid, wid)).collect();
        if panes {
            std::thread::spawn(move || list.into_iter().for_each(|(e, pid, wid)| up(e, pid, wid)));
        } else {
            // nothing to keep in order: all at once, so one slow app delays only itself
            for (e, pid, wid) in list {
                std::thread::spawn(move || up(e, pid, wid));
            }
        }
    }

    /// Remember the stacking while the group is in use.
    fn remember(&mut self, order: &[WinInfo]) {
        let Some(rear) = order.iter().rposition(|w| self.has(w.id).is_some()) else { return };
        let r = self.layout.region();
        self.panes.retain(|p| order.iter().any(|w| w.id == *p && is_small(&w.bounds, &r)));
        self.stack = order[..=rear]
            .iter()
            .filter(|w| self.has(w.id).is_some() || self.panes.contains(&w.id))
            .map(|w| (w.pid, w.id))
            .collect();
    }
}

enum Verdict {
    /// Not ours.
    Pass,
    /// Ours; no app should see it.
    Swallow,
}

impl App {
    fn subscribe(&mut self, pid: i32, elem: &Elem) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.observers.entry(pid) {
            let Some(obs) = Observer::new(pid, callback) else { return };
            e.insert(obs);
        }
        if let Some(obs) = self.observers.get(&pid) {
            obs.add(elem, "AXUIElementDestroyed");
        }
    }

    fn window_closed(&mut self, el: &Elem) {
        let Some((gi, mi)) = self.groups.iter().enumerate().find_map(|(gi, g)| {
            g.members.iter().position(|m| m.elem.same(el)).map(|mi| (gi, mi))
        }) else {
            return;
        };
        self.drag = None;
        let g = &mut self.groups[gi];
        g.members.remove(mi);
        if g.members.len() < 2 {
            self.groups.remove(gi);
            self.leave_chrome_if_idle();
        } else {
            g.layout.remove(mi);
            g.expected.clear();
            g.place_smoothly(&ax::window_order());
        }
    }

    /// Minimum widths the writers found out. Applied even mid-drag, so the seam stops dead.
    fn learn(&mut self) {
        for (wid, width, v) in writer::take_minimums() {
            for g in &mut self.groups {
                if let Some(i) = g.has(wid) {
                    if g.layout.learn_min(i, width, v) {
                        log(&format!("window {wid}: minimum {} {v}", if width { "width" } else { "height" }));
                        if !g.layout.fits() {
                            log("these windows' minimum sizes do not fit together");
                        }
                        g.place();
                    }
                }
            }
        }
    }

    /// The seam under a point, if nothing else is in front of it there.
    fn seam_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        // several groups can have a seam at the same place; the one on top wins below
        let hits: Vec<(usize, usize)> = self
            .groups
            .iter()
            .enumerate()
            .filter_map(|(gi, g)| {
                let k = g.layout.seams().iter().position(|s| {
                    let (across, along) = if s.side { (x, y) } else { (y, x) };
                    (across - s.at).abs() <= GRAB && along >= s.from && along < s.to
                })?;
                Some((gi, k))
            })
            .collect();
        if hits.is_empty() {
            return None;
        }
        let me = std::process::id() as i32;
        let all = ax::all_windows();
        if ax::mission_control_open(&all) {
            return None;
        }
        // an open menu or floating panel above the seam owns the press
        if all.iter().any(|w| w.pid != me && matches!(w.layer, 3 | 8 | 101) && w.bounds.contains(x, y)) {
            return None;
        }
        let top = all.iter().find(|w| w.layer == 0 && w.alpha > 0.05 && w.pid != me && w.bounds.contains(x, y))?;
        hits.into_iter().find(|(gi, _)| self.groups[*gi].has(top.id).is_some())
    }

    fn drag_to(&mut self, x: f64, y: f64) {
        self.learn();
        if let Some((gi, k, off)) = self.drag {
            if let Some(g) = self.groups.get_mut(gi) {
                let Some(before) = g.layout.seams().get(k).copied() else { return };
                g.layout.drag_seam(k, if before.side { x } else { y } - off);
                if g.layout.seams().get(k).map(|s| s.at) != Some(before.at) {
                    g.seam_moved_at = Some(Instant::now());
                    g.place();
                }
            }
        }
    }

    fn mouse(&mut self, kind: u32, x: f64, y: f64, clicks: i64) -> Verdict {
        match kind {
            1 => {
                self.drag = None;
                if self.groups.is_empty() {
                    return Verdict::Pass;
                }
                if let Some((gi, k)) = self.seam_at(x, y) {
                    let g = &mut self.groups[gi];
                    if clicks == 2 {
                        g.layout.equalize();
                        g.place_smoothly(&ax::window_order());
                    }
                    let Some(s) = g.layout.seams().get(k).copied() else { return Verdict::Pass };
                    let off = if s.side { x } else { y } - s.at;
                    self.drag = Some((gi, k, off));
                    return Verdict::Swallow;
                }
                self.verify = None;
                self.mc_pick = None;
                let all = ax::all_windows();
                if ax::mission_control_open(&all) {
                    // the press belongs to Mission Control, but it tells us which group is coming
                    let pick = ax::normal(&all).iter().find(|w| w.bounds.contains(x, y)).map(|w| w.id);
                    self.mc_pick = pick.filter(|w| self.groups.iter().any(|g| g.has(*w).is_some() || g.stack.iter().any(|s| s.1 == *w)));
                    return Verdict::Pass;
                }
                let order = ax::normal(&all);
                if let Some(top) = on_top_at(&order, x, y).map(|w| w.id) {
                    let id = self.groups.iter().find(|g| g.has(top).is_some()).map(|g| g.id);
                    // inside the group in use, a click is macOS's business alone
                    if id.is_some() && id != self.active {
                        self.come_to(id, top);
                    }
                }
                Verdict::Pass
            }
            6 if self.drag.is_some() => {
                self.drag_to(x, y);
                Verdict::Swallow
            }
            2 if self.mc_pick.is_some() => {
                if let Some(wid) = self.mc_pick.take() {
                    let id = self.groups.iter().find(|g| g.stack.iter().any(|s| s.1 == wid) || g.has(wid).is_some()).map(|g| g.id);
                    self.come_to(id, wid);
                }
                Verdict::Pass
            }
            2 if self.drag.is_some() => {
                self.drag_to(x, y);
                self.drag = None;
                Verdict::Swallow
            }
            _ => Verdict::Pass,
        }
    }

    /// The user is coming to a group through this window (a tile, or a pane of it).
    fn come_to(&mut self, id: Option<u32>, wid: u32) {
        self.active = id;
        self.verify = Some((wid, Instant::now(), 0));
        for i in 1..=24 {
            schedule(i as f64 * 0.005, 0.0, follow_cb);
        }
    }

    /// Once the chosen window is up, bring the rest (also while Mission Control is closing).
    fn settle(&mut self, all: &[WinInfo]) -> bool {
        let Some((wid, _, _)) = self.verify else { return false };
        let order = ax::normal(all);
        let mc = ax::mission_control_open(all);
        let active = self.active;
        let Some(top) = order.first().map(|w| w.id) else { return false };
        let Some(g) = self.groups.iter_mut().find(|g| Some(g.id) == active) else { return false };
        // raising a second window of the clicked app can put that one on top; still our group
        if top != wid && g.has(top).is_none() {
            return false;
        }
        if !g.in_place(top, &order, mc) {
            log(&format!(
                "bringing up {} for {top}: order {:?}",
                g.name,
                order.iter().take(8).map(|w| w.id).collect::<Vec<_>>()
            ));
            g.raise_rest(top, &order);
        }
        true
    }

    fn follow(&mut self) {
        if self.verify.is_some_and(|v| v.1.elapsed() < Duration::from_millis(150)) {
            self.settle(&ax::all_windows());
        }
    }

    /// Look again a few times: the choice's own activation lands a moment after ours.
    fn verify_raise(&mut self) {
        const CHECKS: [u64; 6] = [70, 120, 220, 350, 500, 700];
        let Some((wid, t0, n)) = self.verify else { return };
        if n >= CHECKS.len() {
            self.verify = None;
            return;
        }
        if t0.elapsed() < Duration::from_millis(CHECKS[n]) {
            return;
        }
        self.verify = Some((wid, t0, n + 1));
        // something else is on top by now: the user moved on
        if !self.settle(&ax::all_windows()) && n > 1 {
            self.verify = None;
        }
    }

    /// The moment another app comes to the front (Cmd-Tab, Dock), without waiting for a tick.
    fn watch_front(&mut self) {
        let Some(pid) = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()) else { return };
        if pid == self.front_pid {
            return;
        }
        self.front_pid = pid;
        if self.groups.is_empty() || pid == std::process::id() as i32 {
            return;
        }
        let order = ax::window_order();
        if let Some(w) = order.iter().find(|w| w.pid == pid) {
            let w = *w;
            self.note_focus(&w);
        }
    }

    /// Work out which group is in use from the focused window.
    fn note_focus(&mut self, w: &WinInfo) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.has(w.id).is_some()) {
            g.used = w.id;
            let id = g.id;
            if self.active != Some(id) {
                self.come_to(Some(id), w.id);
            }
            return;
        }
        // a small window is a pane over the group; a large one means the user left it
        let Some(g) = self.active.and_then(|id| self.groups.iter_mut().find(|g| g.id == id)) else { return };
        let r = g.layout.region();
        if !is_small(&w.bounds, &r) {
            self.active = None;
        } else if r.contains(w.bounds.x + w.bounds.w / 2.0, w.bounds.y + w.bounds.h / 2.0) && !g.panes.contains(&w.id) {
            g.panes.push(w.id);
        }
    }

    /// Ten times a second. Asks only the window server, so a hung app cannot stall it.
    fn tick(&mut self) {
        self.learn();
        let all = ax::all_windows();
        if ax::mission_control_open(&all) {
            self.mc_seen = Some(Instant::now());
            return;
        }
        let order = ax::normal(&all);
        // windows are still growing back for a moment after Mission Control closes
        let calm = self.mc_seen.is_none_or(|t| t.elapsed() > Duration::from_millis(900));
        let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
        if let Some(pid) = front.filter(|p| *p != std::process::id() as i32) {
            if let Some(w) = order.iter().find(|w| w.pid == pid) {
                let w = *w;
                self.last = Some((pid, w.id));
                if self.verify.is_none() {
                    self.note_focus(&w);
                }
            }
        }
        if ax::left_button_down() || self.drag.is_some() || !calm {
            return;
        }
        for g in &mut self.groups {
            g.correct(&order);
        }
        if self.verify.is_none() {
            let active = self.active;
            if let Some(g) = self.groups.iter_mut().find(|g| Some(g.id) == active) {
                g.remember(&order);
            }
        }
    }

    fn leave_chrome_if_idle(&mut self) {
        if !self.groups.iter().any(|g| g.full) {
            if let Some(s) = self.saved.take() {
                chrome::leave(&s);
            }
        }
    }

    fn target_group(&self) -> Option<usize> {
        let by_focus = self.last.and_then(|(_, wid)| self.groups.iter().position(|g| g.has(wid).is_some()));
        by_focus.or(if self.groups.is_empty() { None } else { Some(0) })
    }

    /// Make a group of these windows, left to right, each keeping its share of the width.
    fn make_group(&mut self, picks: &[(i32, u32)], region: Rect) -> String {
        let order = ax::window_order();
        let found: Vec<(Rect, Member)> = picks
            .iter()
            .filter_map(|&(pid, wid)| {
                let b = order.iter().find(|w| w.id == wid)?.bounds;
                Some((b, Member::open(pid, wid)?))
            })
            .collect();
        if found.len() < 2 {
            return "Need at least two windows that are on screen".into();
        }
        self.drag = None;
        // a window is only ever in one group
        self.groups.retain(|g| !found.iter().any(|(_, m)| g.has(m.wid).is_some()));
        self.leave_chrome_if_idle();

        let rects: Vec<Rect> = found.iter().map(|(b, _)| *b).collect();
        let members: Vec<Member> = found.into_iter().map(|(_, m)| m).collect();
        for m in &members {
            self.subscribe(m.pid, &m.elem);
        }
        let mut g = Group {
            layout: Layout::from_rects(region, &rects),
            members,
            expected: Vec::new(),
            changed_at: Instant::now(),
            seam_moved_at: None,
            id: 0,
            stack: Vec::new(),
            panes: Vec::new(),
            raised_at: None,
            used: 0,
            glide: None,
            name: String::new(),
            full: false,
            windowed: None,
        };
        for other in &mut self.groups {
            other.panes.retain(|p| g.has(*p).is_none());
            other.stack.retain(|p| g.has(p.1).is_none() || other.members.iter().any(|m| m.wid == p.1));
        }
        g.name = default_name(&g);
        g.id = self.next_id;
        g.used = self.last.map_or(0, |l| l.1);
        self.next_id += 1;
        self.active = Some(g.id);
        g.place_smoothly(&order);
        let n = g.members.len();
        self.groups.push(g);
        format!("Tiled {n} windows")
    }
}

extern "C" fn callback(_obs: *mut c_void, el: RawElem, notif: CFStringRef, _refcon: *mut c_void) {
    let name = unsafe { CFString::wrap_under_get_rule(notif) }.to_string();
    if name != "AXUIElementDestroyed" {
        return;
    }
    let el = Elem::from_borrowed(el);
    APP.with(|a| {
        if let Ok(mut a) = a.try_borrow_mut() {
            a.window_closed(&el);
        }
    });
}

// ---- what the overlay draws ----

pub struct GroupView {
    pub region: Rect,
    pub seams: Vec<Seam>,
    /// The seam under the pointer or being dragged.
    pub hot: Option<usize>,
    /// While dragging: the rearmost group window, to put a backdrop behind.
    pub backdrop_below: Option<u32>,
}

pub fn snapshot(mx: f64, my: f64) -> Option<Vec<GroupView>> {
    APP.with(|a| {
        let a = a.try_borrow().ok()?;
        let hot = match a.drag {
            Some((gi, k, _)) => Some((gi, k)),
            None => a.seam_at(mx, my),
        };
        Some(
            a.groups
                .iter()
                .enumerate()
                .map(|(gi, g)| {
                    let mine = hot.filter(|h| h.0 == gi).map(|h| h.1);
                    let moving = g.seam_moved_at.is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                    let rear = moving
                        .then(|| ax::window_order().iter().rev().find(|w| g.has(w.id).is_some()).map(|w| w.id))
                        .flatten();
                    GroupView { region: g.layout.region(), seams: g.layout.seams(), hot: mine, backdrop_below: rear }
                })
                .collect(),
        )
    })
}

// ---- actions called from the menu ----

/// Set once the menu-bar icon image loaded; without it the title carries a text glyph.
pub static HAS_ICON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn title() -> String {
    let glyph = if HAS_ICON.load(std::sync::atomic::Ordering::Relaxed) { "" } else { "▥" };
    with(|a| {
        if a.groups.iter().any(|g| g.full) {
            format!("{glyph} ▣")
        } else if !a.groups.is_empty() {
            format!("{glyph} ●")
        } else {
            glyph.to_string()
        }
    })
}

const NO_PERMISSION: &str =
    "Needs Accessibility permission: add wingroup in System Settings > Privacy & Security > Accessibility, then reopen";

/// The windows "Tile visible windows" would take: on the main display and at least half in view.
pub fn visible_picks() -> Vec<WinInfo> {
    let region = primary_visible();
    let me = std::process::id() as i32;
    let order: Vec<WinInfo> = ax::window_order()
        .into_iter()
        .filter(|w| w.pid != me && w.bounds.w >= 200.0 && w.bounds.h >= 200.0)
        .filter(|w| region.contains(w.bounds.x + w.bounds.w / 2.0, w.bounds.y + w.bounds.h / 2.0))
        .collect();
    let mut picks = Vec::new();
    for (i, w) in order.iter().enumerate() {
        if picks.len() < 2 * MAX_TILES
            && visible_share(&order, i) >= 0.5
            && ax::find_window(w.pid, w.id).is_some_and(|e| e.is_standard_window())
        {
            picks.push(*w);
        }
    }
    picks
}

/// Group the windows you can see, in the order and proportions you arranged them.
pub fn tile_visible() -> String {
    if !ax::is_trusted(false) {
        return NO_PERMISSION.into();
    }
    let seen = visible_picks();
    let grouped = |w: &WinInfo| with(|a| a.groups.iter().any(|g| g.has(w.id).is_some()));
    // windows already in a group stay there; only new ones form a group
    let picks: Vec<(i32, u32)> = seen.iter().filter(|w| !grouped(w)).take(MAX_TILES).map(|w| (w.pid, w.id)).collect();
    if picks.len() < 2 {
        return if picks.len() < seen.len() {
            "Need two ungrouped windows in view (ungroup this one to change it)".into()
        } else {
            "Place at least two windows side by side first".into()
        };
    }
    with(|a| a.make_group(&picks, primary_visible()))
}

/// Tile exactly these windows.
pub fn tile_chosen(picks: &[(i32, u32)]) -> String {
    if !ax::is_trusted(false) {
        return NO_PERMISSION.into();
    }
    let (msg, id) = with(|a| (a.make_group(picks, primary_visible()), a.active));
    // they may have been hidden behind other windows
    if let Some(id) = id.filter(|_| msg.starts_with("Tiled")) {
        switch_to(id);
    }
    msg
}

/// Which group a menu command is about: a given one, or the one in use.
fn pick(a: &App, id: Option<u32>) -> Option<usize> {
    match id {
        Some(id) => a.groups.iter().position(|g| g.id == id),
        None => a.target_group(),
    }
}

pub fn equalize(id: Option<u32>) -> String {
    with(|a| match pick(a, id) {
        Some(i) => {
            a.groups[i].layout.equalize();
            a.groups[i].place_smoothly(&ax::window_order());
            "Sizes evened out".into()
        }
        None => "No group".into(),
    })
}

pub fn ungroup(id: Option<u32>) -> String {
    with(|a| match pick(a, id) {
        Some(i) => {
            a.drag = None;
            a.groups.remove(i);
            a.leave_chrome_if_idle();
            "Ungrouped".into()
        }
        None => "No group".into(),
    })
}

pub fn ungroup_all() -> String {
    with(|a| {
        a.drag = None;
        a.groups.clear();
        a.leave_chrome_if_idle();
        "Ungrouped all".into()
    })
}

/// Bring a group to the front and give its last used window the keyboard.
pub fn switch_to(id: u32) -> String {
    with(|a| {
        let Some(g) = a.groups.iter().find(|g| g.id == id) else { return "No group".into() };
        let m = g.members.iter().find(|m| m.wid == g.used).unwrap_or(&g.members[0]);
        let (pid, wid, elem) = (m.pid, m.wid, m.elem.clone());
        activate(pid, &elem);
        a.come_to(Some(id), wid);
        "Switched".into()
    })
}

fn activate(pid: i32, win: &Elem) {
    win.raise();
    if let Some(app) = Elem::app(pid) {
        app.set_timeout(0.4);
        app.make_frontmost();
    }
}

pub fn toggle_full(id: Option<u32>) -> String {
    with(|a| {
        let Some(i) = pick(a, id) else { return "No group".to_string() };
        if !a.groups[i].full {
            if a.saved.is_none() {
                a.saved = Some(chrome::enter());
            }
            let g = &mut a.groups[i];
            g.windowed = Some(g.layout.region());
            g.full = true;
            g.layout.set_region(ax::main_display_bounds());
            g.place();
            schedule(0.9, 0.0, reapply_cb);
            "Full screen on".into()
        } else {
            let g = &mut a.groups[i];
            g.full = false;
            if let Some(r) = g.windowed.take() {
                g.layout.set_region(r);
                g.place();
            }
            a.leave_chrome_if_idle();
            "Full screen off".into()
        }
    })
}

pub fn shutdown() {
    with(|a| {
        for g in a.groups.iter_mut().filter(|g| g.full) {
            g.full = false;
        }
        a.leave_chrome_if_idle();
    })
}

// ---- timers ----

type TimerFn = extern "C" fn(CFRunLoopTimerRef, *mut c_void);

fn schedule(delay: f64, interval: f64, f: TimerFn) {
    unsafe {
        let t = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + delay,
            interval,
            0,
            0,
            f,
            std::ptr::null_mut(),
        );
        CFRunLoopAddTimer(CFRunLoopGetMain(), t, kCFRunLoopCommonModes);
    }
}

/// Once the Dock and menu bar finished hiding, let the windows use the freed space.
extern "C" fn reapply_cb(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    with(|a| {
        for g in a.groups.iter_mut().filter(|g| g.full) {
            g.layout.set_region(ax::main_display_bounds());
            g.expected.clear();
            g.place();
        }
    })
}

extern "C" fn mouse_tap(_proxy: *mut c_void, kind: u32, event: *mut c_void, _info: *mut c_void) -> *mut c_void {
    if kind == 0xFFFF_FFFE || kind == 0xFFFF_FFFF {
        // a mouse-up may have been missed while the tap was off
        with_try(|a| a.drag = None);
        ax::reenable_tap();
        return event;
    }
    let (x, y) = ax::event_location(event);
    let clicks = ax::click_count(event);
    match with_try(|a| a.mouse(kind, x, y, clicks)).unwrap_or(Verdict::Pass) {
        Verdict::Swallow => {
            crate::overlay::refresh_at(x, y); // the bar moves with the pointer, not a timer later
            return std::ptr::null_mut();
        }
        Verdict::Pass => {}
    }
    event
}

fn with_try<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().map(|mut a| f(&mut a)))
}

extern "C" fn follow_cb(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    with_try(|a| a.follow());
}

extern "C" fn fast_cb(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    with_try(|a| {
        a.counter = a.counter.wrapping_add(1);
        a.verify_raise();
        if a.counter % 2 == 0 {
            a.watch_front();
        }
        for g in &mut a.groups {
            g.step();
        }
        if a.counter % 6 == 0 {
            a.tick();
        }
    });
    if let Some(mtm) = MainThreadMarker::new() {
        let screens = NSScreen::screens(mtm);
        if screens.count() > 0 {
            let p = NSEvent::mouseLocation();
            let h = screens.objectAtIndex(0).frame().size.height;
            crate::overlay::refresh_at(p.x, h - p.y);
            crate::labels::refresh(mtm, h);
        }
    }
}

pub fn start_tracking() {
    schedule(0.1, 0.016, fast_cb);
    let tap = ax::is_trusted(false) && ax::install_click_tap(mouse_tap);
    log(if tap { "tracking started (click tap on)" } else { "tracking started (no click tap; grant Accessibility and reopen)" });
}

// ---- names, and what Mission Control labels need ----

/// The group in use and its current name.
pub fn name_of(id: u32) -> Option<String> {
    with(|a| a.groups.iter().find(|g| g.id == id).map(|g| g.name.clone()))
}

pub fn rename(id: u32, name: &str) -> String {
    with(|a| match a.groups.iter_mut().find(|g| g.id == id) {
        Some(g) if !name.trim().is_empty() => {
            g.name = name.trim().to_string();
            format!("Named “{}”", g.name)
        }
        _ => "Name unchanged".into(),
    })
}

// ---- what the menu shows ----

pub struct Tile {
    pub pid: i32,
}

pub struct MenuGroup {
    pub id: u32,
    pub name: String,
    pub tiles: Vec<Tile>,
    pub active: bool,
    pub full: bool,
}

pub struct MenuWindow {
    pub wid: u32,
    pub pid: i32,
    pub app: String,
    pub title: String,
}

pub struct MenuModel {
    pub trusted: bool,
    /// The windows "Tile" would take, left to right, if it can.
    pub preview: Option<Vec<Tile>>,
    pub all_grouped: bool,
    pub groups: Vec<MenuGroup>,
    /// Every normal window on screen, for choosing by hand.
    pub windows: Vec<MenuWindow>,
}

pub fn app_name(pid: i32) -> String {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .and_then(|a| a.localizedName())
        .map_or("?".into(), |n| n.to_string())
}

pub fn menu_model() -> MenuModel {
    let trusted = ax::is_trusted(false);
    let seen = if trusted { visible_picks() } else { Vec::new() };
    let me = std::process::id() as i32;
    let windows = if trusted {
        ax::window_order()
            .into_iter()
            .filter(|w| w.pid != me && w.bounds.w >= 200.0 && w.bounds.h >= 200.0)
            .filter_map(|w| {
                let e = ax::find_window(w.pid, w.id)?;
                let app = app_name(w.pid);
                let title = e.title().filter(|t| *t != app).unwrap_or_default();
                Some(MenuWindow { wid: w.id, pid: w.pid, app, title })
            })
            .collect()
    } else {
        Vec::new()
    };
    with(|a| {
        let free: Vec<&WinInfo> = seen.iter().filter(|w| !a.groups.iter().any(|g| g.has(w.id).is_some())).take(MAX_TILES).collect();
        let preview = (free.len() >= 2).then(|| {
            let mut v = free.clone();
            v.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x).then(a.bounds.y.total_cmp(&b.bounds.y)));
            v.iter().map(|w| Tile { pid: w.pid }).collect()
        });
        let groups = a
            .groups
            .iter()
            .map(|g| MenuGroup {
                id: g.id,
                name: g.name.clone(),
                tiles: g.members.iter().map(|m| Tile { pid: m.pid }).collect(),
                active: a.active == Some(g.id),
                full: g.full,
            })
            .collect();
        MenuModel { trusted, preview, all_grouped: free.is_empty() && !seen.is_empty(), groups, windows }
    })
}

pub struct Label {
    pub wid: u32,
    /// The window's thumbnail.
    pub at: Rect,
    pub name: String,
    pub pids: Vec<i32>,
}

/// While Mission Control is open: one label per grouped window on screen.
pub fn mission_control_labels() -> Option<Vec<Label>> {
    with_try(|a| {
        // the tick notices Mission Control; only look closely around then
        if a.groups.is_empty() || !a.mc_seen.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return Some(Vec::new());
        }
        let all = ax::all_windows();
        if !ax::mission_control_open(&all) || a.verify.is_some() || a.mc_pick.is_some() {
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        for g in &a.groups {
            let mut pids: Vec<i32> = g.members.iter().map(|m| m.pid).collect();
            pids.dedup();
            for m in &g.members {
                if let Some(w) = all.iter().find(|w| w.id == m.wid) {
                    out.push(Label { wid: m.wid, at: w.bounds, name: g.name.clone(), pids: pids.clone() });
                }
            }
        }
        Some(out)
    })?
}
