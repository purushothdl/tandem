//! Window groups: what is in them, where the seams are, and the only code allowed to change that.
//!
//! The layout is the truth. It changes when the user drags a seam (which this app
//! handles itself), uses the menu, or an app reports a minimum width. A window found
//! anywhere else is put back; it never reshapes the group.

use crate::ax::{self, Elem, Observer, RawElem, WinInfo};
use crate::chrome;
use crate::layout::{presets, Layout, Preset, Rect, Seam};
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
const GLIDE: f64 = 0.30;

struct Member {
    elem: Elem,
    pid: i32,
    wid: u32,
    tx: Sender<Rect>,
    /// A frame the app keeps coming back to; asking again would be pointless.
    stuck_at: Option<Rect>,
    /// Where it was before it joined, to return to when it leaves.
    home: Option<Rect>,
}

impl Member {
    fn open(pid: i32, wid: u32) -> Option<Member> {
        let elem = ax::find_window(pid, wid)?;
        elem.set_timeout(0.4);
        let home = elem.frame();
        let tx = writer::spawn(elem.clone(), wid);
        Some(Member { elem, pid, wid, tx, stuck_at: None, home })
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
    handed_at: Option<Instant>,
    members: Vec<Member>,
    layout: Layout,
    expected: Vec<Rect>,
    changed_at: Instant,
    /// The minimum sizes known before the seam drag going on.
    mins_before_drag: Option<Vec<(f64, f64)>>,
    /// Put the windows in place at this moment (after a drop).
    settle_at: Option<Instant>,
    /// Last time the user actually moved a seam.
    seam_moved_at: Option<Instant>,
    /// Windows gliding to `expected`: where each started, and when.
    glide: Option<(Vec<Rect>, Instant)>,
    /// The layout picked last, to step on from.
    preset: usize,
    name: String,
    /// The user chose the name.
    named: bool,
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
    /// A group's card was clicked: until Mission Control is closed and the group is in front.
    card: Option<CardPick>,
    /// The window on top in Mission Control; a change means one was picked.
    mc_top: u32,
    front_pid: i32,
    /// A press on a tile that may become a title-bar drag.
    press: Option<Press>,
    /// A dropped pane about to join: when, group, pid, window, where it was before the drag.
    joining: Option<(Instant, u32, i32, u32, Rect, Join)>,
    /// In Mission Control: what was on top when it opened (pid, window id).
    mc_origin: Option<(i32, u32)>,
    /// The window being hovered there, and since when.
    mc_hover: Option<(u32, Instant)>,
    /// The group brought up in advance for the hovered window.
    prepared: Option<u32>,
    /// Diagnostics: window order after a switch, logged on change.
    trace: Option<(Instant, Vec<u32>, bool)>,
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
        card: None,
        mc_top: 0,
        front_pid: 0,
        press: None,
        joining: None,
        mc_origin: None,
        mc_hover: None,
        prepared: None,
        trace: None,
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

fn raise_all<'a>(members: impl Iterator<Item = &'a Member>) {
    for m in members {
        let e = writer::SendElem(m.elem.clone());
        std::thread::spawn(move || {
            let e = e;
            if !e.0.is_minimized() {
                e.0.raise();
            }
        });
    }
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

/// The whole display below the menu bar. (Under the translucent menu bar, anything we draw
/// while resizing would shine through it.)
fn full_region() -> Rect {
    let d = ax::main_display_bounds();
    let top = primary_visible().y.max(d.y);
    Rect { y: top, h: d.bottom() - top, ..d }
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
            if self.expected.get(j) != Some(f) && !self.layout.is_hidden(j) {
                let _ = m.tx.send(*f);
            }
        }
        self.expected = frames;
        self.changed_at = Instant::now();
    }

    /// The same, but let the windows glide there from where they are now.
    /// `still`: a window that is already in place (its reported position is mid-animation).
    fn place_smoothly(&mut self, order: &[WinInfo], still: Option<u32>) {
        let to = self.layout.frames();
        let from: Vec<Rect> = self
            .members
            .iter()
            .zip(&to)
            .enumerate()
            .map(|(j, (m, t))| {
                if self.layout.is_hidden(j) || Some(m.wid) == still {
                    return *t;
                }
                order.iter().find(|w| w.id == m.wid).map_or(*t, |w| w.bounds)
            })
            .collect();
        let far = from.iter().zip(&to).map(|(a, b)| a.distance(b)).fold(0.0, f64::max);
        let calm = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
        if far > 24.0 && !calm {
            // only ever the steps: sending the end first would make the windows jump
            self.members.iter_mut().for_each(|m| m.stuck_at = None);
            self.expected = to;
            self.changed_at = Instant::now();
            self.glide = Some((from, Instant::now()));
        } else {
            self.expected.clear(); // ask every window again, wherever it is
            self.place();
        }
    }

    /// One frame of the glide: eases in and out.
    fn step(&mut self) {
        let Some((from, t0)) = &self.glide else { return };
        let t = (t0.elapsed().as_secs_f64() / GLIDE).min(1.0);
        let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
        for (j, ((m, a), b)) in self.members.iter().zip(from).zip(&self.expected).enumerate() {
            if !self.layout.is_hidden(j) {
                let _ = m.tx.send(a.lerp(b, e));
            }
        }
        self.changed_at = Instant::now();
        if t >= 1.0 {
            self.glide = None;
        }
    }

    fn set_full(&mut self, on: bool) {
        if on == self.full {
            return;
        }
        self.full = on;
        if on {
            self.windowed = Some(self.layout.region());
            self.layout.set_region(full_region());
        } else if let Some(r) = self.windowed.take() {
            self.layout.set_region(r);
        }
    }

    /// The group in another shape. Big spots go to the tile in use; the others keep their order.
    fn arranged(&self, p: &Preset) -> Layout {
        let frames = self.layout.frames();
        let mut ids: Vec<usize> = (0..self.members.len()).collect();
        ids.sort_by(|a, b| {
            let (a, b) = (&frames[*a], &frames[*b]);
            (a.x + a.w / 4.0).total_cmp(&(b.x + b.w / 4.0)).then(a.y.total_cmp(&b.y))
        });
        if p.main {
            if let Some(u) = self.has(self.used) {
                ids.retain(|i| *i != u);
                ids.insert(0, u);
            }
        }
        let mut rects = vec![Rect::default(); ids.len()];
        for (slot, i) in ids.into_iter().enumerate() {
            rects[i] = p.rects[slot];
        }
        self.layout.rearranged(&rects)
    }

    fn set_preset(&mut self, k: usize, order: &[WinInfo]) -> bool {
        let Some(p) = presets(self.members.len()).get(k).cloned() else { return false };
        let l = self.arranged(&p);
        if !l.fits() {
            return false;
        }
        self.preset = k;
        self.layout = l;
        self.place_smoothly(order, None);
        true
    }

    /// Keep the automatic name in step, unless the user chose one.
    fn auto_name(&mut self) {
        if !self.named {
            self.name = default_name(self);
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
        for (j, (m, exp)) in self.members.iter_mut().zip(&self.expected).enumerate() {
            if self.layout.is_hidden(j) {
                continue;
            }
            let Some(w) = order.iter().find(|w| w.id == m.wid) else { continue };
            if w.bounds.near(exp) {
                m.stuck_at = None;
            } else if m.stuck_at != Some(w.bounds) {
                // what is on screen may be an animation (minimising, Spaces); the app knows
                if !m.elem.is_minimized() && m.elem.frame().is_some_and(|f| !f.near(exp)) {
                    off = true;
                }
            }
        }
        if off {
            let seen: Vec<Option<Rect>> = self.members.iter().map(|m| order.iter().find(|w| w.id == m.wid).map(|w| w.bounds)).collect();
            self.place_smoothly(order, None);
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
    fn raise_rest(&mut self, focus: u32, order: &[WinInfo]) -> bool {
        if self.raised_at.is_some_and(|t| t.elapsed() < Duration::from_millis(50)) {
            return false;
        }
        // macOS keeps all windows of the active app above other apps' raised windows. When
        // another window of the focused app sits in between, activate each app in turn instead.
        let fpid = order.iter().find(|w| w.id == focus).map(|w| w.pid);
        let want_ids = self.wanted(focus, order);
        let rear = order.iter().rposition(|w| want_ids.iter().any(|s| s.1 == w.id)).unwrap_or(0);
        let sibling = order[..rear].iter().any(|w| {
            Some(w.pid) == fpid && w.id != focus && !want_ids.iter().any(|s| s.1 == w.id) && self.has(w.id).is_none()
        });
        if sibling && want_ids.iter().all(|s| Some(s.0) != fpid) {
            if self.handed_at.is_some_and(|t| t.elapsed() < Duration::from_millis(45)) {
                return false;
            }
            let Some(f) = self.members.iter().find(|m| m.wid == focus) else { return false };
            self.handed_at = Some(Instant::now());
            let mut list: Vec<(writer::SendElem, i32, u32)> = want_ids
                .iter()
                .rev()
                .filter_map(|s| self.members.iter().find(|m| m.wid == s.1))
                .map(|m| (writer::SendElem(m.elem.clone()), m.pid, m.wid))
                .collect();
            list.push((writer::SendElem(f.elem.clone()), f.pid, f.wid));
            std::thread::spawn(move || {
                let mut list = list;
                let Some((f, fpid, fwid)) = list.pop() else { return };
                for (e, pid, _) in list {
                    if !e.0.is_minimized() {
                        activate(pid, &e.0);
                    }
                }
                // back to the chosen window: first the app without its windows, then just this one
                if !ax::focus_without_raising(fpid, fwid) {
                    activate(fpid, &f.0);
                }
                f.0.raise();
            });
            return true;
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
        true
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

struct Press {
    group: u32,
    wid: u32,
    /// Where the pointer and the window were.
    at: (f64, f64),
    from: Rect,
    /// The window is really being dragged (not text selected in it).
    moving: bool,
    looked: Instant,
    /// The tile it would trade places with.
    over: Option<u32>,
    /// Not in a group: with Shift held it joins this one.
    pid: i32,
    pane: bool,
    join: Option<Join>,
}

/// The tile a dragged pane would share with, and what it would get.
#[derive(Clone, Copy)]
struct Join {
    tile: u32,
    /// Left or above, rather than right or below.
    before: bool,
    half: Rect,
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
            for n in ["AXUIElementDestroyed", "AXWindowMiniaturized", "AXWindowDeminiaturized"] {
                obs.add(elem, n);
            }
        }
    }

    fn window_closed(&mut self, el: &Elem) {
        let Some((gi, mi)) = self.groups.iter().enumerate().find_map(|(gi, g)| {
            g.members.iter().position(|m| m.elem.same(el)).map(|mi| (gi, mi))
        }) else {
            return;
        };
        self.drop_member(gi, mi);
    }

    /// Minimised: the neighbours take the space. Back: it gets its place again.
    fn window_hidden(&mut self, el: &Elem, hidden: bool) {
        let Some((g, mi)) = self.groups.iter_mut().find_map(|g| g.members.iter().position(|m| m.elem.same(el)).map(|mi| (g, mi))) else {
            return;
        };
        if g.layout.is_hidden(mi) == hidden {
            return;
        }
        self.drag = None;
        g.layout.set_hidden(mi, hidden);
        let wid = g.members[mi].wid;
        g.place_smoothly(&ax::window_order(), Some(wid));
    }

    /// Take a window out of its group; the neighbour gets its space.
    fn drop_member(&mut self, gi: usize, mi: usize) {
        self.drag = None;
        let g = &mut self.groups[gi];
        let wid = g.members.remove(mi).wid;
        g.stack.retain(|s| s.1 != wid);
        if g.members.len() < 2 {
            self.groups.remove(gi);
            self.leave_chrome_if_idle();
        } else {
            g.layout.remove(mi);
            g.auto_name();
            g.place_smoothly(&ax::window_order(), None);
        }
    }

    /// Add windows to a group, splitting the tile last used.
    fn add_to(&mut self, id: u32, picks: &[(i32, u32)]) -> String {
        let order = ax::window_order();
        let mut spots: Vec<u32> = Vec::new(); // tiles we may split: the one in use, and what we add
        let mut added = 0;
        let mut refused = None;
        for &(pid, wid) in picks {
            if self.groups.iter().any(|g| g.id == id && g.has(wid).is_some()) {
                continue;
            }
            let Some(g) = self.groups.iter().find(|g| g.id == id) else { return "No group".into() };
            if g.members.len() >= MAX_TILES {
                refused = Some(format!("No more room: a group holds {MAX_TILES} windows"));
                break;
            }
            let Some(m) = Member::open(pid, wid) else { continue };
            let frames = g.layout.frames();
            let area = |i: usize| frames[i].w * frames[i].h;
            let largest = |v: &[usize]| v.iter().copied().max_by(|a, b| area(*a).total_cmp(&area(*b)));
            if spots.is_empty() {
                spots.push(g.members[g.has(g.used).unwrap_or(0)].wid);
            }
            let near: Vec<usize> = spots.iter().filter_map(|w| g.has(*w)).collect();
            let all: Vec<usize> = (0..g.members.len()).collect();
            // where the user works; failing that, the largest tile
            let Some(layout) = [largest(&near), largest(&all)].into_iter().flatten().find_map(|i| {
                let mut l = g.layout.clone();
                l.split(i);
                l.fits().then_some(l)
            }) else {
                refused = Some(format!("No room for {}: the tiles would get too small", app_name(pid)));
                continue;
            };
            // a window is only ever in one group
            if let Some((gi, mi)) = self.groups.iter().enumerate().find_map(|(gi, g)| g.has(wid).map(|mi| (gi, mi))) {
                self.drop_member(gi, mi);
            }
            for other in &mut self.groups {
                other.panes.retain(|p| *p != wid);
                other.stack.retain(|s| s.1 != wid);
            }
            self.subscribe(pid, &m.elem);
            let Some(g) = self.groups.iter_mut().find(|g| g.id == id) else { return "No group".into() };
            g.layout = layout;
            g.members.push(m);
            spots.push(wid);
            added += 1;
        }
        if added > 0 {
            self.drag = None;
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
                g.auto_name();
                g.place_smoothly(&order, None);
            }
        }
        refused.unwrap_or_else(|| format!("Added {added}"))
    }

    /// Minimum widths the writers found out. Applied even mid-drag, so the seam stops dead.
    fn learn(&mut self) {
        let dragging = self.drag.map(|d| d.0);
        for m in writer::take_minimums() {
            for (gi, g) in self.groups.iter_mut().enumerate() {
                let Some(i) = g.has(m.wid) else { continue };
                if m.sure {
                    if let Some(x) = g.mins_before_drag.as_mut().and_then(|v| v.get_mut(i)) {
                        let x = if m.width { &mut x.0 } else { &mut x.1 };
                        *x = x.max(m.size);
                    }
                } else if dragging != Some(gi) {
                    continue;
                }
                if g.layout.learn_min(i, m.width, m.size) {
                    let what = if m.width { "width" } else { "height" };
                    log(&format!("window {}: minimum {what} {}{}", m.wid, m.size, if m.sure { "" } else { " (for this drag)" }));
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

    /// The seam under a point, if nothing else is in front of it there.
    fn seam_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        // nothing behind one of our own dialogs reacts
        if MODAL.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        // several groups can have a seam at the same place; the one on top wins below
        let hits: Vec<(usize, usize)> = self
            .groups
            .iter()
            .enumerate()
            // only the group in use, even where another one shows around its edges
            .filter(|(_, g)| self.active.is_none_or(|id| id == g.id))
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

    fn mouse(&mut self, kind: u32, x: f64, y: f64, clicks: i64, shift: bool) -> Verdict {
        match kind {
            1 => {
                self.drag = None;
                if self.groups.is_empty() {
                    return Verdict::Pass;
                }
                // a press on one of our own dialogs is not a press on the tile behind it
                if MODAL.load(std::sync::atomic::Ordering::Relaxed) {
                    let me = std::process::id() as i32;
                    if ax::all_windows().iter().any(|w| w.pid == me && w.bounds.h > 60.0 && w.bounds.w > 60.0 && w.bounds.contains(x, y)) {
                        return Verdict::Pass;
                    }
                }
                if let Some((gi, k)) = self.seam_at(x, y) {
                    let g = &mut self.groups[gi];
                    if clicks == 2 {
                        g.layout.equalize();
                        g.place_smoothly(&ax::window_order(), None);
                    }
                    let Some(s) = g.layout.seams().get(k).copied() else { return Verdict::Pass };
                    let off = if s.side { x } else { y } - s.at;
                    g.mins_before_drag = Some(g.layout.mins().to_vec());
                    self.drag = Some((gi, k, off));
                    return Verdict::Swallow;
                }
                self.verify = None;
                self.press = None;
                let all = ax::all_windows();
                if ax::mission_control_open(&all) {
                    // Mission Control's business (and real clicks there do not even reach us);
                    // watch_mission_control sees what gets picked
                    return Verdict::Pass;
                }
                let order = ax::normal(&all);
                if let Some(top) = on_top_at(&order, x, y).map(|w| w.id) {
                    let id = self.groups.iter().find(|g| g.has(top).is_some()).map(|g| g.id);
                    // inside the group in use, a click is macOS's business alone
                    if id.is_some() && id != self.active {
                        self.come_to(id, top);
                    }
                    // a window of no group may be dragged into the one in use
                    let pane = id.is_none();
                    let group = id.or(self.active);
                    if let (Some(group), Some(w)) = (group, order.iter().find(|w| w.id == top)) {
                        self.press = Some(Press {
                            group,
                            wid: top,
                            at: (x, y),
                            from: w.bounds,
                            moving: false,
                            looked: Instant::now(),
                            over: None,
                            pid: w.pid,
                            pane,
                            join: None,
                        });
                    }
                }
                Verdict::Pass
            }
            6 if self.press.is_some() => {
                self.drag_tile(x, y, shift);
                Verdict::Pass
            }
            2 if self.press.is_some() => {
                self.drop_tile(x, y, shift);
                Verdict::Pass
            }
            6 if self.drag.is_some() => {
                self.drag_to(x, y);
                Verdict::Swallow
            }
            2 if self.drag.is_some() => {
                self.drag_to(x, y);
                self.drag = None;
                Verdict::Swallow
            }
            _ => Verdict::Pass,
        }
    }

    /// Hovering a window in Mission Control. If its app also has windows elsewhere, macOS
    /// would keep those above the rest of its group until the other apps answer, so bring
    /// them up now: thumbnails do not overlap, and the click then finds everything in place.
    fn hover_in_mission_control(&mut self, wid: Option<u32>, order: &[WinInfo]) {
        if self.mc_hover.map(|h| h.0) != wid {
            self.mc_hover = wid.map(|w| (w, Instant::now()));
            return;
        }
        let Some((wid, since)) = self.mc_hover else { return };
        if since.elapsed() < Duration::from_millis(90) {
            return;
        }
        let Some(w) = order.iter().find(|w| w.id == wid).copied() else { return };
        let Some(g) = self.groups.iter().find(|g| g.has(wid).is_some()) else {
            self.unprepare();
            return;
        };
        let id = g.id;
        if self.prepared == Some(id) {
            return;
        }
        let elsewhere = order.iter().any(|o| o.pid == w.pid && g.has(o.id).is_none());
        // the group already in front needs nothing
        if !elsewhere || (self.prepared.is_none() && self.active == Some(id)) {
            self.unprepare();
            return;
        }
        log(&format!("preparing {} for {wid}", g.name));
        // raise only: activating anything here would change the front app and restack the
        // thumbnails of its windows
        raise_all(g.members.iter().filter(|m| m.pid != w.pid));
        self.prepared = Some(id);
    }

    /// Put things back as they were when Mission Control opened.
    fn unprepare(&mut self) {
        if self.prepared.take().is_none() {
            return;
        }
        log("unprepare");
        let Some((pid, wid)) = self.mc_origin else { return };
        if ax::window_order().first().map(|w| w.id) != Some(wid) {
            return; // something else was chosen after all
        }
        if let Some(g) = self.groups.iter().find(|g| g.has(wid).is_some()) {
            raise_all(g.members.iter().filter(|m| m.pid != pid));
        } else if let Some(e) = ax::find_window(pid, wid) {
            e.raise();
        }
    }

    /// A tile is perhaps being dragged by its title bar: note which tile is under the pointer.
    fn drag_tile(&mut self, x: f64, y: f64, shift: bool) {
        let Some(p) = &mut self.press else { return };
        if !p.moving {
            // only now and then: this runs for every drag anywhere in a tile
            if (x - p.at.0).hypot(y - p.at.1) < 12.0 || p.looked.elapsed() < Duration::from_millis(40) {
                return;
            }
            p.looked = Instant::now();
            let Some(b) = ax::window_order().iter().find(|w| w.id == p.wid).map(|w| w.bounds) else { return };
            // the window follows the pointer; a resized or still window does not
            let (dx, dy) = (b.x - p.from.x, b.y - p.from.y);
            if dx.hypot(dy) < 8.0 || (b.right() - p.from.right()).abs() < 1.0 || (b.bottom() - p.from.bottom()).abs() < 1.0 && dy.abs() > 1.0 {
                return;
            }
            p.moving = true;
        }
        let g = self.groups.iter().find(|g| g.id == p.group);
        if p.pane {
            p.join = g.filter(|_| shift).and_then(|g| {
                let frames = g.layout.frames();
                let i = (0..g.members.len()).find(|&i| !g.layout.is_hidden(i) && frames[i].contains(x, y))?;
                // the half the pointer is in
                let r = frames[i];
                let side = r.w >= r.h;
                let before = if side { x < r.x + r.w / 2.0 } else { y < r.y + r.h / 2.0 };
                let mut l = g.layout.clone();
                let new = l.insert(i, side, before);
                l.fits().then(|| Join { tile: g.members[i].wid, before, half: l.frames()[new] })
            });
            return;
        }
        p.over = g.and_then(|g| {
            let frames = g.layout.frames();
            (0..g.members.len()).find(|&i| !g.layout.is_hidden(i) && frames[i].contains(x, y)).map(|i| g.members[i].wid)
        });
        if p.over == Some(p.wid) {
            p.over = None;
        }
    }

    /// A pane was dropped on a tile with Shift held.
    fn join(&mut self) {
        let Some((_, id, pid, wid, from, j)) = self.joining.take_if(|j| j.0 <= Instant::now()) else { return };
        let Some(g) = self.groups.iter().find(|g| g.id == id) else { return };
        let Some(i) = g.has(j.tile) else { return };
        if g.members.len() >= MAX_TILES {
            log("no join: the group is full");
            return;
        }
        let mut layout = g.layout.clone();
        let r = layout.frames()[i];
        layout.insert(i, r.w >= r.h, j.before);
        if !layout.fits() {
            log("no join: it does not fit there");
            return;
        }
        let Some(mut m) = Member::open(pid, wid) else { return };
        m.home = Some(from); // where it was before it was dragged here
        self.subscribe(pid, &m.elem);
        for g in &mut self.groups {
            g.panes.retain(|p| *p != wid);
            g.stack.retain(|s| s.1 != wid);
        }
        let Some(g) = self.groups.iter_mut().find(|g| g.id == id) else { return };
        g.layout = layout;
        g.members.push(m);
        g.used = wid;
        g.auto_name();
        g.place_smoothly(&ax::window_order(), None);
        self.come_to(Some(id), wid);
    }

    /// Let go: trade places with the tile under the pointer, or go back.
    fn drop_tile(&mut self, x: f64, y: f64, shift: bool) {
        self.drag_tile(x, y, shift);
        let Some(p) = self.press.take().filter(|p| p.moving) else { return };
        if p.pane {
            if let Some(j) = p.join {
                // once the app has seen the release
                self.joining = Some((Instant::now() + Duration::from_millis(110), p.group, p.pid, p.wid, p.from, j));
            }
            return;
        }
        let Some(g) = self.groups.iter_mut().find(|g| g.id == p.group) else { return };
        if let (Some(i), Some(j)) = (g.has(p.wid), p.over.and_then(|w| g.has(w))) {
            if g.layout.swap(i, j) {
                g.members.swap(i, j);
                g.auto_name();
            } else {
                log("no swap: one of them does not fit in the other's place");
            }
        }
        // the app has not seen the release yet and would refuse to be resized
        g.changed_at = Instant::now();
        g.settle_at = Some(Instant::now() + Duration::from_millis(110));
    }

    /// The user is coming to a group through this window (a tile, or a pane of it).
    fn come_to(&mut self, id: Option<u32>, wid: u32) {
        self.active = id;
        self.trace = Some((Instant::now(), Vec::new(), false));
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
        if !g.in_place(top, &order, mc) && g.raise_rest(top, &order) {
            log(&format!("bringing up {} for {top} at {} ms", g.name, self.verify.map_or(0, |v| v.1.elapsed().as_millis())));
        }
        true
    }

    fn follow(&mut self) {
        if self.verify.is_some_and(|v| v.1.elapsed() < Duration::from_millis(150)) {
            self.settle(&ax::all_windows());
        }
    }

    fn trace_order(&mut self) {
        let Some((t0, last, mc0)) = &mut self.trace else { return };
        if t0.elapsed() > Duration::from_millis(1200) {
            self.trace = None;
            return;
        }
        let all = ax::all_windows();
        let mc = ax::mission_control_open(&all);
        let now: Vec<u32> = ax::normal(&all).iter().take(7).map(|w| w.id).collect();
        if now != *last || mc != *mc0 {
            log(&format!("  +{} ms mc {mc} front {}: {now:?}", t0.elapsed().as_millis(), self.front_pid));
            *last = now;
            *mc0 = mc;
        }
    }

    /// Picking a window in Mission Control puts it on top at once, long before Mission
    /// Control has closed. That is the earliest sign there is, and the only one when the
    /// window belongs to the app that was already active.
    fn watch_mission_control(&mut self) {
        if self.groups.is_empty() || !self.mc_seen.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        let all = ax::all_windows();
        if !ax::mission_control_open(&all) {
            return;
        }
        let Some(top) = ax::normal(&all).first().map(|w| w.id) else { return };
        if top == self.mc_top {
            return;
        }
        self.mc_top = top;
        self.prepared = None;
        let id = self.groups.iter().find(|g| g.has(top).is_some()).map(|g| g.id);
        let pane = || self.groups.iter().find(|g| g.stack.iter().any(|s| s.1 == top)).map(|g| g.id);
        let id = id.or_else(pane);
        log(&format!("picked {top} in Mission Control, group {id:?}"));
        if id.is_some() && !self.verify.is_some_and(|v| v.0 == top) {
            self.come_to(id, top);
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
        // during a switch, an app of that group becoming active is part of the switch; its
        // first window may still be one that belongs elsewhere
        if self.verify.is_some() {
            let active = self.active;
            if self.groups.iter().any(|g| Some(g.id) == active && g.members.iter().any(|m| m.pid == pid)) {
                return;
            }
        }
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
            if self.mc_seen.is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
                self.mc_origin = ax::normal(&all).first().map(|w| (w.pid, w.id));
                self.mc_top = self.mc_origin.map_or(0, |o| o.1);
                self.mc_hover = None;
                self.prepared = None;
            }
            self.mc_seen = Some(Instant::now());
            return;
        }
        let order = ax::normal(&all);
        if self.prepared.is_some() {
            self.unprepare(); // left without choosing
            return;
        }
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
            settle_at: None,
            mins_before_drag: None,
            seam_moved_at: None,
            id: 0,
            stack: Vec::new(),
            panes: Vec::new(),
            raised_at: None,
            handed_at: None,
            used: 0,
            glide: None,
            preset: 0,
            name: String::new(),
            named: false,
            full: false,
            windowed: None,
        };
        for other in &mut self.groups {
            other.panes.retain(|p| g.has(*p).is_none());
            other.stack.retain(|p| g.has(p.1).is_none() || other.members.iter().any(|m| m.wid == p.1));
        }
        // the windowed size must not be measured while the Dock is hidden
        if let Some(r) = self.groups.iter().find(|o| o.full).map(|o| o.windowed) {
            if let Some(r) = r {
                g.layout.set_region(r);
            }
            g.set_full(true);
        }
        g.name = default_name(&g);
        g.id = self.next_id;
        g.used = self.last.map_or(0, |l| l.1);
        self.next_id += 1;
        self.active = Some(g.id);
        g.place_smoothly(&order, None);
        let n = g.members.len();
        self.groups.push(g);
        format!("Tiled {n} windows")
    }
}

extern "C" fn callback(_obs: *mut c_void, el: RawElem, notif: CFStringRef, _refcon: *mut c_void) {
    let name = unsafe { CFString::wrap_under_get_rule(notif) }.to_string();
    let el = Elem::from_borrowed(el);
    with_try(|a| match name.as_str() {
        "AXUIElementDestroyed" => a.window_closed(&el),
        "AXWindowMiniaturized" => a.window_hidden(&el, true),
        "AXWindowDeminiaturized" => a.window_hidden(&el, false),
        _ => {}
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
    /// The tile a dragged one would trade places with.
    pub drop: Option<Rect>,
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
                    let over = a.press.as_ref().filter(|p| p.moving && p.group == g.id).and_then(|p| p.over);
                    let join = a.press.as_ref().filter(|p| p.moving && p.group == g.id).and_then(|p| p.join).map(|j| j.half);
                    let drop = join.or(over.and_then(|w| g.has(w)).and_then(|i| g.expected.get(i).copied()));
                    let moving = g.seam_moved_at.is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                    let rear = moving
                        .then(|| ax::window_order().iter().rev().find(|w| g.has(w.id).is_some()).map(|w| w.id))
                        .flatten();
                    GroupView { region: g.layout.region(), seams: g.layout.seams(), hot: mine, backdrop_below: rear, drop }
                })
                .collect(),
        )
    })
}

// ---- actions called from the menu ----

/// One of our dialogs is open.
pub static MODAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

/// Add these windows to a group.
pub fn add_windows(id: u32, picks: &[(i32, u32)]) -> String {
    let msg = with(|a| a.add_to(id, picks));
    if msg.starts_with("Added") {
        switch_to(id);
    }
    msg
}

pub fn remove_window(wid: u32) -> String {
    with(|a| {
        let Some((gi, mi)) = a.groups.iter().enumerate().find_map(|(gi, g)| g.has(wid).map(|mi| (gi, mi))) else {
            return "No such window in a group".into();
        };
        // back to the size it had before, and behind the group it no longer belongs to
        let (tx, home) = (a.groups[gi].members[mi].tx.clone(), a.groups[gi].members[mi].home);
        let next = a.groups[gi]
            .members
            .iter()
            .enumerate()
            .filter(|(i, m)| *i != mi && m.wid != wid && !a.groups[gi].layout.is_hidden(*i))
            .max_by_key(|(_, m)| m.wid == a.groups[gi].used)
            .map(|(_, m)| (m.pid, m.wid, m.elem.clone()));
        let id = a.groups[gi].id;
        a.groups[gi].panes.retain(|p| *p != wid);
        a.drop_member(gi, mi);
        if let Some((pid, w, elem)) = next {
            activate(pid, &elem);
            a.come_to(a.groups.iter().any(|g| g.id == id).then_some(id), w);
        }
        if let Some(h) = home {
            // once the group is in front of it
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(350));
                let _ = tx.send(h);
            });
        }
        "Removed".into()
    })
}

/// Which group a menu command is about: a given one, or the one in use.
fn pick(a: &App, id: Option<u32>) -> Option<usize> {
    match id {
        Some(id) => a.groups.iter().position(|g| g.id == id),
        None => a.target_group(),
    }
}

pub fn set_layout(id: u32, k: usize) -> String {
    let msg = with(|a| {
        a.drag = None;
        match a.groups.iter_mut().find(|g| g.id == id) {
            Some(g) => {
                if g.set_preset(k, &ax::window_order()) {
                    "Layout changed"
                } else {
                    "No room for that layout"
                }
            }
            None => "No group",
        }
    });
    if msg == "Layout changed" {
        switch_to(id);
    }
    msg.into()
}

/// The next layout that fits and looks different.
pub fn next_layout() -> String {
    with(|a| {
        a.drag = None;
        let Some(g) = a.target_group().and_then(|i| a.groups.get_mut(i)) else { return "No group".into() };
        let order = ax::window_order();
        let n = presets(g.members.len()).len();
        let now = g.layout.frames();
        let same = |l: &Layout| l.frames().iter().zip(&now).all(|(a, b)| a.distance(b) < 24.0);
        let next = (1..=n).map(|s| (g.preset + s) % n.max(1)).find(|&k| {
            presets(g.members.len()).get(k).is_some_and(|p| {
                let l = g.arranged(p);
                l.fits() && !same(&l)
            })
        });
        match next {
            Some(k) if g.set_preset(k, &order) => "Next layout".into(),
            _ => "No other layout fits".into(),
        }
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
        let shown = |i: usize| !g.layout.is_hidden(i);
        let m = g.members.iter().enumerate().filter(|(i, _)| shown(*i)).max_by_key(|(_, m)| m.wid == g.used).map_or(&g.members[0], |(_, m)| m);
        let (pid, wid, elem) = (m.pid, m.wid, m.elem.clone());
        activate(pid, &elem);
        a.come_to(Some(id), wid);
        "Switched".into()
    })
}

#[derive(Clone, Copy)]
struct CardPick {
    group: u32,
    at: Instant,
    /// When Mission Control was first seen closed.
    closed: Option<Instant>,
    again: Option<Instant>,
}

/// A group's card in Mission Control was pressed. The press also closes Mission Control.
pub fn pick_card(id: u32) {
    log(&format!("card of group {id} pressed"));
    with(|a| {
        a.prepared = None;
        a.card = Some(CardPick { group: id, at: Instant::now(), closed: None, again: None });
    });
    switch_to(id);
}

/// Closing Mission Control can hand the front back to the window it was opened from.
fn finish_card() {
    let again = with_try(|a| {
        let mut c = a.card?;
        let all = ax::all_windows();
        let open = ax::mission_control_open(&all);
        if c.closed.is_none() && !open {
            c.closed = Some(Instant::now());
        }
        let done = c.closed.is_some_and(|t| t.elapsed() > Duration::from_millis(350));
        if done || c.at.elapsed() > Duration::from_millis(1800) {
            a.card = None;
            return None;
        }
        let top = ax::normal(&all).first().map(|w| w.id)?;
        let up = a.groups.iter().find(|g| g.id == c.group)?.has(top).is_some();
        let wait = c.again.is_some_and(|t| t.elapsed() < Duration::from_millis(150));
        let go = c.closed.is_some() && !up && !wait;
        if go {
            c.again = Some(Instant::now());
            log(&format!("card: {top} is in front instead, switching again"));
        }
        a.card = Some(c);
        go.then_some(c.group)
    });
    if let Some(Some(id)) = again {
        switch_to(id);
    }
}

fn activate(pid: i32, win: &Elem) {
    win.raise();
    if let Some(app) = Elem::app(pid) {
        app.set_timeout(0.4);
        app.make_frontmost();
    }
}

/// Full screen is for all groups at once: the Dock it hides is system-wide.
pub fn toggle_full() -> String {
    with(|a| {
        let on = !a.groups.iter().any(|g| g.full);
        let order = ax::window_order();
        if on && a.saved.is_none() {
            a.saved = Some(chrome::enter());
        }
        for g in &mut a.groups {
            g.set_full(on);
            g.place_smoothly(&order, None);
        }
        a.drag = None;
        if on {
            schedule(0.9, 0.0, reapply_cb);
        } else {
            a.leave_chrome_if_idle();
        }
        if on { "Full screen on".into() } else { "Full screen off".into() }
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
            g.layout.set_region(full_region());
            g.expected.clear();
            g.place();
        }
    })
}

extern "C" fn mouse_tap(_proxy: *mut c_void, kind: u32, event: *mut c_void, _info: *mut c_void) -> *mut c_void {
    if kind == 0xFFFF_FFFE || kind == 0xFFFF_FFFF {
        // a mouse-up may have been missed while the tap was off
        with_try(|a| {
            a.drag = None;
            a.press = None;
        });
        ax::reenable_tap();
        return event;
    }
    let (x, y) = ax::event_location(event);
    let clicks = ax::click_count(event);
    let shift = ax::shift_held(event);
    match with_try(|a| a.mouse(kind, x, y, clicks, shift)).unwrap_or(Verdict::Pass) {
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
        a.watch_mission_control();
        a.trace_order();
        a.verify_raise();
        if a.counter % 2 == 0 {
            a.watch_front();
        }
        a.join();
        for g in &mut a.groups {
            if g.settle_at.is_some_and(|t| t <= Instant::now()) {
                g.settle_at = None;
                g.place_smoothly(&ax::window_order(), None);
            }
            g.step();
        }
        if a.counter % 6 == 0 {
            a.tick();
        }
    });
    finish_card();
    if let Some(mtm) = MainThreadMarker::new() {
        let screens = NSScreen::screens(mtm);
        if screens.count() > 0 {
            let p = NSEvent::mouseLocation();
            let h = screens.objectAtIndex(0).frame().size.height;
            crate::overlay::refresh_at(p.x, h - p.y);
            crate::labels::refresh(mtm, p.x, h - p.y, h);
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

pub fn tiles_of(id: u32) -> Vec<Tile> {
    with(|a| a.groups.iter().filter(|g| g.id == id).flat_map(|g| &g.members).map(|m| Tile { pid: m.pid, wid: m.wid }).collect())
}

pub fn rename(id: u32, name: &str) -> String {
    with(|a| match a.groups.iter_mut().find(|g| g.id == id) {
        Some(g) if !name.trim().is_empty() => {
            g.name = name.trim().to_string();
            g.named = true;
            format!("Named “{}”", g.name)
        }
        _ => "Name unchanged".into(),
    })
}

// ---- what the menu shows ----

pub struct Tile {
    pub pid: i32,
    pub wid: u32,
}

pub struct LayoutChoice {
    /// As parts of a whole; the first is the big spot, if there is one.
    pub rects: Vec<Rect>,
    pub main: bool,
    pub fits: bool,
    pub current: bool,
}

pub struct MenuGroup {
    pub layouts: Vec<LayoutChoice>,
    pub id: u32,
    pub name: String,
    pub tiles: Vec<Tile>,
    pub active: bool,
}

pub struct MenuWindow {
    pub wid: u32,
    pub pid: i32,
    pub app: String,
    pub title: String,
}

pub struct MenuModel {
    pub full: bool,
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
            v.iter().map(|w| Tile { pid: w.pid, wid: w.id }).collect()
        });
        let groups = a
            .groups
            .iter()
            .map(|g| MenuGroup {
                layouts: presets(g.members.len())
                    .into_iter()
                    .map(|p| {
                        let l = g.arranged(&p);
                        let shape = |l: &Layout| {
                            let mut f = l.frames();
                            f.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
                            f
                        };
                        let current = shape(&l).iter().zip(shape(&g.layout)).all(|(a, b)| a.distance(&b) < 24.0);
                        LayoutChoice { fits: l.fits(), current, main: p.main, rects: p.rects }
                    })
                    .collect(),
                id: g.id,
                name: g.name.clone(),
                tiles: g.members.iter().map(|m| Tile { pid: m.pid, wid: m.wid }).collect(),
                active: a.active == Some(g.id),
            })
            .collect();
        let full = a.groups.iter().any(|g| g.full);
        MenuModel { full, trusted, preview, all_grouped: free.is_empty() && !seen.is_empty(), groups, windows }
    })
}

pub struct Label {
    pub wid: u32,
    pub group: u32,
    /// The pointer is on it.
    pub hovered: bool,
    /// The window's thumbnail.
    pub at: Rect,
    pub name: String,
    pub pids: Vec<i32>,
}

/// While Mission Control is open: one label per grouped window on screen.
pub fn mission_control_labels(mx: f64, my: f64) -> Option<Vec<Label>> {
    with_try(|a| {
        // the tick notices Mission Control; only look closely around then
        if a.groups.is_empty() || !a.mc_seen.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return Some(Vec::new());
        }
        let all = ax::all_windows();
        if !ax::mission_control_open(&all) || a.verify.is_some() {
            return Some(Vec::new());
        }
        // thumbnails of one app overlap, so only the topmost under the pointer counts
        let order = ax::normal(&all);
        let top = on_top_at(&order, mx, my).map(|w| w.id);
        a.hover_in_mission_control(top, &order);
        let mut out = Vec::new();
        for g in &a.groups {
            let mut pids: Vec<i32> = g.members.iter().map(|m| m.pid).collect();
            pids.dedup();
            for m in &g.members {
                if let Some(w) = all.iter().find(|w| w.id == m.wid) {
                    out.push(Label { wid: m.wid, at: w.bounds, hovered: top == Some(w.id), group: g.id, name: g.name.clone(), pids: pids.clone() });
                }
            }
        }
        Some(out)
    })?
}

/// One group, as a card in Mission Control.
#[derive(Clone, PartialEq)]
pub struct Card {
    pub group: u32,
    pub name: String,
    /// Each tile as parts of the whole, and its app.
    pub tiles: Vec<(Rect, i32)>,
}

/// A card per group with a window on screen, the group in use first, and the places taken
/// by thumbnails and what Mission Control draws on them.
pub fn mission_control_cards() -> Option<(Vec<Card>, Vec<Rect>)> {
    with_try(|a| {
        let all = ax::all_windows();
        let order = ax::normal(&all);
        let d = primary_bounds(&all)?;
        let me = std::process::id() as i32;
        let mut cards: Vec<Card> = a
            .groups
            .iter()
            .filter(|g| order.iter().any(|w| g.has(w.id).is_some()))
            .map(|g| {
                let r = g.layout.region();
                let part = |f: &Rect| Rect { x: (f.x - r.x) / r.w, y: (f.y - r.y) / r.h, w: f.w / r.w, h: f.h / r.h };
                let frames = g.layout.frames();
                let tiles = (frames.iter().zip(&g.members).enumerate())
                    .filter(|(i, (f, _))| !g.layout.is_hidden(*i) && f.w > 1.0 && f.h > 1.0)
                    .map(|(_, (f, m))| (part(f), m.pid))
                    .collect();
                Card { group: g.id, name: g.name.clone(), tiles }
            })
            .collect();
        cards.sort_by_key(|c| a.active != Some(c.group));
        let small = |w: &WinInfo| w.layer == 17 && w.bounds.w < d.w * 0.5;
        let taken = order.iter().chain(all.iter().filter(|w| small(w))).filter(|w| w.pid != me).map(|w| w.bounds).collect();
        Some((cards, taken))
    })?
}

fn primary_bounds(all: &[WinInfo]) -> Option<Rect> {
    all.iter().find(|w| w.layer == 18).map(|w| w.bounds)
}
