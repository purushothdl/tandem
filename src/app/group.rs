//! A group: its windows, its layout, and putting the windows where the layout says.

use super::{GroupId, writer};
use crate::core::geometry::Rect;
use crate::core::glide::Glide;
use crate::core::layout::{Layout, Preset, presets};
use crate::core::window::{self, Window, WindowRef};
use crate::platform::element::{Element, find_window};
use crate::platform::{apps, screen};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Shorter moves are not worth animating.
const GLIDE_FROM: f64 = 24.0;
/// Leave windows alone this long after moving them.
const SETTLE: Duration = Duration::from_millis(400);

pub(super) struct Member {
    pub element: Element,
    pub pid: i32,
    pub wid: u32,
    /// Frames go to the thread that owns this window.
    pub frames: Sender<Rect>,
    /// A frame the app keeps coming back to; asking again would be pointless.
    pub stuck_at: Option<Rect>,
    /// Where it was before it joined, to return to when it leaves.
    pub home: Option<Rect>,
}

impl Member {
    pub fn open(w: WindowRef) -> Option<Member> {
        let element = find_window(w.pid, w.wid)?;
        let home = element.frame();
        let frames = writer::spawn(element.clone(), w.wid);
        Some(Member { element, pid: w.pid, wid: w.wid, frames, stuck_at: None, home })
    }

    pub fn reference(&self) -> WindowRef {
        WindowRef { pid: self.pid, wid: self.wid }
    }
}

pub(super) struct Group {
    pub id: GroupId,
    pub name: String,
    /// The user chose the name.
    pub named: bool,
    /// In the same order as the layout's windows.
    pub members: Vec<Member>,
    pub layout: Layout,
    /// The layout picked last, to step on from.
    pub preset: usize,
    /// The tile last used, to return the keyboard to.
    pub used: u32,

    /// Front to back as last seen in use: tiles, and panes in front of any of them.
    pub stack: Vec<WindowRef>,
    /// Small windows the user opened or clicked over this group; nothing else is a pane.
    pub panes: Vec<u32>,
    pub raised_at: Option<Instant>,
    pub handed_over_at: Option<Instant>,

    /// Where each window was last told to be.
    pub expected: Vec<Rect>,
    pub changed_at: Instant,
    pub glide: Option<Glide>,
    /// Put the windows in place then (after a drop).
    pub settle_at: Option<Instant>,
    /// Last time the user actually moved a seam.
    pub seam_moved_at: Option<Instant>,
    /// Minimum sizes known before the seam drag going on.
    pub mins_before_drag: Option<Vec<(f64, f64)>>,

    pub full: bool,
    /// The region before full screen.
    pub windowed: Option<Rect>,
}

impl Group {
    pub fn new(id: GroupId, members: Vec<Member>, layout: Layout) -> Group {
        let mut g = Group {
            id,
            name: String::new(),
            named: false,
            members,
            layout,
            preset: 0,
            used: 0,
            stack: Vec::new(),
            panes: Vec::new(),
            raised_at: None,
            handed_over_at: None,
            expected: Vec::new(),
            changed_at: Instant::now(),
            glide: None,
            settle_at: None,
            seam_moved_at: None,
            mins_before_drag: None,
            full: false,
            windowed: None,
        };
        g.update_name();
        g
    }

    pub fn has(&self, wid: u32) -> bool {
        self.index_of(wid).is_some()
    }

    pub fn index_of(&self, wid: u32) -> Option<usize> {
        self.members.iter().position(|m| m.wid == wid)
    }

    pub fn is_shown(&self, i: usize) -> bool {
        !self.layout.is_hidden(i)
    }

    /// The tile showing at a point.
    pub fn tile_at(&self, x: f64, y: f64) -> Option<usize> {
        let frames = self.layout.frames();
        (0..self.members.len()).find(|&i| self.is_shown(i) && frames[i].contains(x, y))
    }

    /// The member to give the keyboard to: the one last used, if it is showing.
    pub fn main_member(&self) -> &Member {
        let shown = self.members.iter().enumerate().filter(|(i, _)| self.is_shown(*i));
        shown.max_by_key(|(_, m)| m.wid == self.used).map_or(&self.members[0], |(_, m)| m)
    }

    /// It is not a pane of this group any more.
    pub fn forget(&mut self, wid: u32) {
        self.panes.retain(|p| *p != wid);
        self.stack.retain(|s| s.wid != wid);
    }

    /// Keep the automatic name in step, unless the user chose one.
    pub fn update_name(&mut self) {
        if !self.named {
            let pids: Vec<i32> = self.members.iter().map(|m| m.pid).collect();
            self.name = super::views::app_names(&pids);
        }
    }

    // ---- putting windows in place ----

    /// At once.
    pub fn place(&mut self) {
        self.glide = None;
        let frames = self.layout.frames();
        for (i, (m, f)) in self.members.iter_mut().zip(&frames).enumerate() {
            m.stuck_at = None;
            if self.expected.get(i) != Some(f) && !self.layout.is_hidden(i) {
                let _ = m.frames.send(*f);
            }
        }
        self.expected = frames;
        self.changed_at = Instant::now();
    }

    /// Let the windows glide there from where they are now. `still`: a window that is already
    /// in place (its reported position is mid-animation).
    pub fn place_smoothly(&mut self, order: &[Window], still: Option<u32>) {
        let to = self.layout.frames();
        let from: Vec<Rect> = (self.members.iter().zip(&to).enumerate())
            .map(|(i, (m, t))| {
                if !self.is_shown(i) || Some(m.wid) == still {
                    return *t;
                }
                window::find(order, m.wid).map_or(*t, |w| w.bounds)
            })
            .collect();
        let far = from.iter().zip(&to).map(|(a, b)| a.distance(b)).fold(0.0, f64::max);
        let no_motion = apps::reduce_motion();
        if far > GLIDE_FROM && !no_motion {
            // only ever the steps: sending the end first would make the windows jump
            self.members.iter_mut().for_each(|m| m.stuck_at = None);
            self.expected = to;
            self.changed_at = Instant::now();
            self.glide = Some(Glide::new(from, Instant::now()));
        } else {
            self.expected.clear(); // ask every window again, wherever it is
            self.place();
        }
    }

    /// One frame of the glide.
    pub fn step(&mut self) {
        let Some(glide) = &self.glide else { return };
        let now = Instant::now();
        let t = glide.progress(now);
        for (i, ((m, a), b)) in self.members.iter().zip(&glide.from).zip(&self.expected).enumerate() {
            if self.is_shown(i) {
                let _ = m.frames.send(a.lerp(b, t));
            }
        }
        self.changed_at = now;
        if glide.is_done(now) {
            self.glide = None;
        }
    }

    /// Put back any window that is not where the layout says.
    pub fn correct(&mut self, order: &[Window]) {
        if self.glide.is_some() || self.changed_at.elapsed() < SETTLE {
            return;
        }
        let mut off = false;
        for (i, (m, expected)) in self.members.iter_mut().zip(&self.expected).enumerate() {
            if self.layout.is_hidden(i) {
                continue;
            }
            let Some(w) = window::find(order, m.wid) else { continue };
            if w.bounds.near(expected) {
                m.stuck_at = None;
            } else if m.stuck_at != Some(w.bounds) {
                // what is on screen may be an animation (minimising, Spaces); the app knows
                off |= !m.element.is_minimized() && m.element.frame().is_some_and(|f| !f.near(expected));
            }
        }
        if off {
            let seen: Vec<Option<Rect>> =
                self.members.iter().map(|m| window::find(order, m.wid).map(|w| w.bounds)).collect();
            self.place_smoothly(order, None);
            for ((m, seen), expected) in self.members.iter_mut().zip(seen).zip(&self.expected) {
                m.stuck_at = seen.filter(|b| !b.near(expected));
            }
        }
    }

    // ---- other shapes ----

    /// The group in another shape. A big spot goes to the tile in use; the others keep their
    /// order.
    pub fn arranged(&self, p: &Preset) -> Layout {
        let frames = self.layout.frames();
        let mut ids: Vec<usize> = (0..self.members.len()).collect();
        ids.sort_by(|a, b| {
            let (a, b) = (&frames[*a], &frames[*b]);
            (a.x + a.w / 4.0).total_cmp(&(b.x + b.w / 4.0)).then(a.y.total_cmp(&b.y))
        });
        if p.main
            && let Some(used) = self.index_of(self.used)
        {
            ids.retain(|i| *i != used);
            ids.insert(0, used);
        }
        let mut rects = vec![Rect::default(); ids.len()];
        for (slot, i) in ids.into_iter().enumerate() {
            rects[i] = p.rects[slot];
        }
        self.layout.rearranged(&rects)
    }

    /// False if its windows cannot get that small.
    pub fn set_preset(&mut self, k: usize, order: &[Window]) -> bool {
        let Some(p) = presets(self.members.len()).get(k).cloned() else { return false };
        let layout = self.arranged(&p);
        if !layout.fits() {
            return false;
        }
        self.preset = k;
        self.layout = layout;
        self.place_smoothly(order, None);
        true
    }

    pub fn set_full(&mut self, on: bool) {
        if on == self.full {
            return;
        }
        self.full = on;
        if on {
            self.windowed = Some(self.layout.region());
            self.layout.set_region(screen::below_menu_bar());
        } else if let Some(r) = self.windowed.take() {
            self.layout.set_region(r);
        }
    }
}
