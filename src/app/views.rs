//! What the UI draws: plain data, copied out of the groups.

use super::tiles::Target;
use super::{APP, App, GroupId, MAX_TILES, commands, own_pid, try_with, with};
use crate::core::geometry::Rect;
use crate::core::layout::{Layout, Seam, presets};
use crate::core::preview;
use crate::core::window::{self, Window, WindowRef};
use crate::platform::element::{find_window, is_trusted};
use crate::platform::{apps, screen, windows};
use std::time::Duration;

/// The backdrop stays this long after a seam last moved.
const RESIZING: Duration = Duration::from_millis(400);

/// "Code + Chrome"
pub fn app_names(pids: &[i32]) -> String {
    let mut names: Vec<String> = Vec::new();
    for n in pids.iter().map(|p| apps::name(*p)) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names.join(" + ")
}

pub fn name_of(id: GroupId) -> Option<String> {
    with(|a| a.group(id).map(|g| g.name.clone()))
}

pub fn pids_of(id: GroupId) -> Vec<i32> {
    with(|a| a.group(id).map_or(Vec::new(), |g| g.members.iter().map(|m| m.pid).collect()))
}

/// For the menu bar.
pub struct Status {
    pub any: bool,
    pub full: bool,
}

pub fn status() -> Status {
    with(|a| Status { any: !a.groups.is_empty(), full: a.groups.iter().any(|g| g.full) })
}

// ---- over the windows ----

pub struct Overlay {
    pub region: Rect,
    pub seams: Vec<Seam>,
    /// The seam under the pointer or being dragged.
    pub hot: Option<usize>,
    /// While resizing: the rearmost group window, to put a backdrop behind.
    pub backdrop_below: Option<u32>,
    /// Where a dragged window would go.
    pub drop: Option<Rect>,
}

pub fn overlays(x: f64, y: f64) -> Option<Vec<Overlay>> {
    APP.with(|a| {
        let a = a.try_borrow().ok()?;
        let hot = match a.seam_drag {
            Some(d) => Some((d.group, d.seam)),
            None => a.seam_at(x, y),
        };
        Some(a.groups.iter().map(|g| overlay(&a, g, hot)).collect())
    })
}

fn overlay(a: &App, g: &super::group::Group, hot: Option<(GroupId, usize)>) -> Overlay {
    let press = a.press.as_ref().filter(|p| p.moving && p.group == g.id);
    let drop = press.and_then(|p| match &p.target {
        Target::Swap { with } => g.expected.get(g.index_of((*with)?)?).copied(),
        Target::Join(half) => half.map(|h| h.rect),
    });
    let resizing = g.seam_moved_at.is_some_and(|t| t.elapsed() < RESIZING);
    let rearmost = || windows::ordinary().iter().rev().find(|w| g.has(w.id)).map(|w| w.id);
    Overlay {
        region: g.layout.region(),
        seams: g.layout.seams(),
        hot: hot.filter(|h| h.0 == g.id).map(|h| h.1),
        backdrop_below: if resizing { rearmost() } else { None },
        drop,
    }
}

// ---- the menu ----

pub struct LayoutChoice {
    /// As parts of a whole; the first is the big spot, if there is one.
    pub rects: Vec<Rect>,
    pub main: bool,
    pub fits: bool,
    pub current: bool,
}

pub struct MenuGroup {
    pub id: GroupId,
    pub name: String,
    pub tiles: Vec<WindowRef>,
    pub layouts: Vec<LayoutChoice>,
    pub active: bool,
}

pub struct MenuWindow {
    pub window: WindowRef,
    pub app: String,
    pub title: String,
}

#[derive(Default)]
pub struct Menu {
    pub trusted: bool,
    pub full: bool,
    /// The windows Tile would take, left to right, if it can.
    pub preview: Option<Vec<WindowRef>>,
    pub all_grouped: bool,
    pub groups: Vec<MenuGroup>,
    /// Every normal window on screen, for choosing by hand.
    pub windows: Vec<MenuWindow>,
}

pub fn menu() -> Menu {
    if !is_trusted(false) {
        return Menu::default();
    }
    let seen = commands::visible_picks();
    let windows = all_windows();
    with(|a| {
        let free: Vec<&Window> = seen.iter().filter(|w| a.group_of(w.id).is_none()).take(MAX_TILES).collect();
        Menu {
            trusted: true,
            full: a.groups.iter().any(|g| g.full),
            preview: (free.len() >= 2).then(|| left_to_right(&free)),
            all_grouped: free.is_empty() && !seen.is_empty(),
            groups: a.groups.iter().map(|g| menu_group(a, g)).collect(),
            windows,
        }
    })
}

fn all_windows() -> Vec<MenuWindow> {
    let big = |w: &Window| w.bounds.w >= commands::SMALLEST_WINDOW && w.bounds.h >= commands::SMALLEST_WINDOW;
    (windows::ordinary().iter())
        .filter(|w| w.pid != own_pid() && big(w))
        .filter_map(|w| {
            let e = find_window(w.pid, w.id)?;
            let app = apps::name(w.pid);
            let title = e.title().filter(|t| *t != app).unwrap_or_default();
            Some(MenuWindow { window: w.reference(), app, title })
        })
        .collect()
}

fn left_to_right(windows: &[&Window]) -> Vec<WindowRef> {
    let mut v = windows.to_vec();
    v.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x).then(a.bounds.y.total_cmp(&b.bounds.y)));
    v.into_iter().map(Window::reference).collect()
}

fn menu_group(a: &App, g: &super::group::Group) -> MenuGroup {
    // the same tiles, whichever window is in which
    let shape = |l: &Layout| {
        let mut f = l.frames();
        f.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        f
    };
    let now = shape(&g.layout);
    let choice = |p: crate::core::layout::Preset| {
        let l = g.arranged(&p);
        let current = shape(&l).iter().zip(&now).all(|(a, b)| a.distance(b) < 24.0);
        LayoutChoice { fits: l.fits(), current, main: p.main, rects: p.rects }
    };
    MenuGroup {
        id: g.id,
        name: g.name.clone(),
        tiles: g.members.iter().map(|m| m.reference()).collect(),
        layouts: presets(g.members.len()).into_iter().map(choice).collect(),
        active: a.active == Some(g.id),
    }
}

// ---- Mission Control ----

/// A grouped window's thumbnail.
pub struct Thumbnail {
    pub wid: u32,
    pub group: GroupId,
    pub at: Rect,
    /// The pointer is on it.
    pub hovered: bool,
    pub name: String,
    pub pids: Vec<i32>,
}

/// One per grouped window on screen; none while Mission Control is closed.
pub fn thumbnails(x: f64, y: f64) -> Option<Vec<Thumbnail>> {
    try_with(|a| {
        if a.groups.is_empty() || !a.mission_control.recent() {
            return Vec::new();
        }
        let all = windows::all();
        if !windows::mission_control_open(&all) || a.switch.is_some() {
            return Vec::new();
        }
        // thumbnails of one app overlap, so only the topmost under the pointer counts
        let order = window::ordinary(&all);
        let top = window::on_top_at(&order, x, y).map(|w| w.id);
        a.hover_in_mission_control(top, &order);
        let mut out = Vec::new();
        for g in &a.groups {
            let mut pids: Vec<i32> = g.members.iter().map(|m| m.pid).collect();
            pids.dedup();
            for w in g.members.iter().filter_map(|m| window::find(&all, m.wid)) {
                let (name, pids) = (g.name.clone(), pids.clone());
                out.push(Thumbnail { wid: w.id, group: g.id, at: w.bounds, hovered: top == Some(w.id), name, pids });
            }
        }
        out
    })
}

/// An entry of the bar.
#[derive(Clone, PartialEq)]
pub struct BarEntry {
    pub group: GroupId,
    pub name: String,
    /// Each tile as parts of the whole, and its app.
    pub tiles: Vec<(Rect, i32)>,
}

pub struct Bar {
    /// Groups with a window on screen, the one in use first.
    pub entries: Vec<BarEntry>,
    /// Where thumbnails and app icons are.
    pub taken: Vec<Rect>,
    /// Every window shown.
    pub windows: Vec<u32>,
    /// Of all thumbnails together: shrinks while Mission Control opens, grows as it closes.
    pub area: f64,
}

pub fn bar() -> Option<Bar> {
    try_with(|a| {
        let all = windows::all();
        let order = window::ordinary(&all);
        let display = screen::bounds();
        let on_screen = a.groups.iter().filter(|g| order.iter().any(|w| g.has(w.id)));
        let mut entries: Vec<BarEntry> = on_screen.map(bar_entry).collect();
        entries.sort_by_key(|e| a.active != Some(e.group));
        let icons = all.iter().filter(|w| w.is_mission_control_icon(&display));
        let taken = order.iter().chain(icons).filter(|w| w.pid != own_pid()).map(|w| w.bounds).collect();
        let mut windows: Vec<u32> = order.iter().map(|w| w.id).collect();
        windows.sort_unstable();
        Bar { entries, taken, windows, area: order.iter().map(|w| w.bounds.area()).sum() }
    })
}

fn bar_entry(g: &super::group::Group) -> BarEntry {
    let r = g.layout.region();
    let part = |f: &Rect| Rect { x: (f.x - r.x) / r.w, y: (f.y - r.y) / r.h, w: f.w / r.w, h: f.h / r.h };
    let frames = g.layout.frames();
    let tiles = (frames.iter().zip(&g.members).enumerate())
        .filter(|(i, (f, _))| g.is_shown(*i) && f.w > 1.0 && f.h > 1.0)
        .map(|(_, (f, m))| (part(f), m.pid))
        .collect();
    BarEntry { group: g.id, name: g.name.clone(), tiles: preview::even(tiles) }
}
