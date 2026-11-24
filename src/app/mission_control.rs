//! Following what happens in Mission Control.
//!
//! Real clicks there reach neither our event tap nor our windows, so a pick is noticed from
//! the window order changing.

use super::{App, GroupId, commands, try_with, with};
use crate::core::card_pick::{CardPick, Step};
use crate::core::window::{self, Window, WindowRef};
use crate::platform::element::find_window;
use crate::platform::log::log;
use crate::platform::windows;
use std::time::{Duration, Instant};

/// Not seen for this long: closed since.
const GONE: Duration = Duration::from_millis(250);
/// Not seen for this long: the next sighting is a new opening.
const REOPENED: Duration = Duration::from_millis(400);
/// Windows are still growing back this long after it closes.
const CALM_AFTER: Duration = Duration::from_millis(900);
/// Pointing at a window this long is a hover.
const HOVER: Duration = Duration::from_millis(90);

#[derive(Default)]
pub(super) struct MissionControl {
    seen: Option<Instant>,
    /// What was on top when it opened.
    origin: Option<WindowRef>,
    /// What is on top now; a change means a window was picked.
    top: u32,
    /// The window pointed at, and since when.
    hover: Option<(u32, Instant)>,
    /// The group raised in advance for it.
    prepared: Option<GroupId>,
    card: Option<CardPick>,
}

impl MissionControl {
    pub fn recent(&self) -> bool {
        self.seen.is_some_and(|t| t.elapsed() < GONE)
    }

    pub fn calm(&self) -> bool {
        self.seen.is_none_or(|t| t.elapsed() > CALM_AFTER)
    }

    pub fn has_prepared(&self) -> bool {
        self.prepared.is_some()
    }
}

impl App {
    /// Is it open? Keeps track of it opening.
    pub(super) fn note_mission_control(&mut self, all: &[Window]) -> bool {
        if !windows::mission_control_open(all) {
            return false;
        }
        let mc = &mut self.mission_control;
        if mc.seen.is_none_or(|t| t.elapsed() > REOPENED) {
            mc.origin = window::ordinary(all).first().map(Window::reference);
            mc.top = mc.origin.map_or(0, |o| o.wid);
            mc.hover = None;
            mc.prepared = None;
        }
        mc.seen = Some(Instant::now());
        true
    }

    /// A picked window goes on top at once, long before Mission Control has closed. That is
    /// the earliest sign there is, and the only one when its app was already active.
    pub(super) fn watch_mission_control(&mut self) {
        if self.groups.is_empty() || !self.mission_control.recent() {
            return;
        }
        let all = windows::all();
        if !windows::mission_control_open(&all) {
            return;
        }
        let Some(top) = window::ordinary(&all).first().map(|w| w.id) else { return };
        if top == self.mission_control.top {
            return;
        }
        self.mission_control.top = top;
        self.mission_control.prepared = None;
        let pane_of = || self.groups.iter().find(|g| g.stack.iter().any(|s| s.wid == top));
        let id = self.group_of(top).or_else(pane_of).map(|g| g.id);
        log(&format!("picked {top} in Mission Control, group {id:?}"));
        if id.is_some() && !self.switch.is_some_and(|s| s.window == top) {
            self.come_to(id, top);
        }
    }

    /// Pointing at a window whose app also has windows elsewhere. macOS would keep those above
    /// the rest of its group until the other apps answer, so raise them now: thumbnails do not
    /// overlap, and the click then finds everything in place.
    pub(super) fn hover_in_mission_control(&mut self, wid: Option<u32>, order: &[Window]) {
        let mc = &mut self.mission_control;
        if mc.hover.map(|h| h.0) != wid {
            mc.hover = wid.map(|w| (w, Instant::now()));
            return;
        }
        let Some((wid, _)) = mc.hover.filter(|(_, since)| since.elapsed() >= HOVER) else { return };
        let Some(w) = window::find(order, wid) else { return };
        let Some(g) = self.group_of(wid) else {
            self.unprepare();
            return;
        };
        let (id, prepared) = (g.id, self.mission_control.prepared);
        if prepared == Some(id) {
            return;
        }
        let elsewhere = order.iter().any(|o| o.pid == w.pid && !g.has(o.id));
        // the group already in front needs nothing
        if !elsewhere || (prepared.is_none() && self.active == Some(id)) {
            self.unprepare();
            return;
        }
        // raise only: activating anything here would restack the thumbnails
        g.raise_except(w.pid);
        self.mission_control.prepared = Some(id);
    }

    /// Put things back as they were when Mission Control opened.
    pub(super) fn unprepare(&mut self) {
        if self.mission_control.prepared.take().is_none() {
            return;
        }
        let Some(origin) = self.mission_control.origin else { return };
        if windows::ordinary().first().map(|w| w.id) != Some(origin.wid) {
            return; // something else was chosen after all
        }
        if let Some(g) = self.group_of(origin.wid) {
            g.raise_except(origin.pid);
        } else if let Some(e) = find_window(origin.pid, origin.wid) {
            e.raise();
        }
    }
}

/// An entry of the bar was pressed. The press also closes Mission Control.
pub fn press_bar_entry(id: GroupId) {
    with(|a| {
        a.mission_control.prepared = None;
        a.mission_control.card = Some(CardPick::new(id, Instant::now()));
    });
    commands::report(commands::switch_to(id));
}

/// See that the group picked from the bar stays in front.
pub(super) fn follow_bar_pick() {
    let again = try_with(|a| {
        let group = a.mission_control.card?.group;
        let all = windows::all();
        let Some(g) = a.group(group) else {
            a.mission_control.card = None;
            return None;
        };
        let in_front = g.has(window::ordinary(&all).first()?.id);
        let card = a.mission_control.card.as_mut()?;
        match card.step(Instant::now(), windows::mission_control_open(&all), in_front) {
            Step::Wait => None,
            Step::SwitchAgain => Some(group),
            Step::Done => {
                a.mission_control.card = None;
                None
            }
        }
    });
    if let Some(id) = again.flatten() {
        commands::report(commands::switch_to(id));
    }
}
