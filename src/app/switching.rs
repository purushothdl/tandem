//! Coming to a group: one of its windows is up, the rest must follow.

use super::{App, GroupId, own_pid, try_with};
use crate::core::recheck::{Recheck, Step};
use crate::core::window::{self, Window};
use crate::platform::log::log;
use crate::platform::{apps, timer, windows};
use std::time::{Duration, Instant};

/// Right after a switch, look every few milliseconds for the chosen window to come up.
const FOLLOW_EVERY: Duration = Duration::from_millis(5);
const FOLLOW_LOOKS: u32 = 24;
const FOLLOW_FOR: Duration = Duration::from_millis(150);

impl App {
    /// The user is coming to a group through this window (a tile, or a pane of it).
    pub(super) fn come_to(&mut self, group: Option<GroupId>, wid: u32) {
        self.active = group;
        self.switch = Some(Recheck::new(wid, Instant::now()));
        for i in 1..=FOLLOW_LOOKS {
            timer::after(FOLLOW_EVERY * i, || {
                try_with(App::follow);
            });
        }
    }

    fn follow(&mut self) {
        if self.switch.is_some_and(|s| s.elapsed(Instant::now()) < FOLLOW_FOR) {
            self.bring_up_rest(&windows::all());
        }
    }

    /// Later looks, with growing delays.
    pub(super) fn recheck_switch(&mut self) {
        let Some(switch) = &mut self.switch else { return };
        match switch.step(Instant::now()) {
            Step::Wait => {}
            Step::Finished => self.switch = None,
            Step::Check => {
                let give_up = switch.give_up();
                if !self.bring_up_rest(&windows::all()) && give_up {
                    self.switch = None; // the user moved on
                }
            }
        }
    }

    /// Once the chosen window is up, bring the rest, also while Mission Control is closing.
    /// False if something else is on top.
    fn bring_up_rest(&mut self, all: &[Window]) -> bool {
        let Some(switch) = self.switch else { return false };
        let order = window::ordinary(all);
        let thumbnails = windows::mission_control_open(all);
        let Some(top) = order.first().map(|w| w.id) else { return false };
        let Some(g) = self.active.and_then(|id| self.group_mut(id)) else { return false };
        // raising a second window of the clicked app can put that one on top; still our group
        if top != switch.window && !g.has(top) {
            return false;
        }
        if !g.in_place(top, &order, thumbnails) && g.raise_rest(top, &order) {
            let ms = switch.elapsed(Instant::now()).as_millis();
            log(&format!("bringing up {} for {top} at {ms} ms", g.name));
        }
        true
    }

    /// The moment another app comes to the front (Cmd-Tab, Dock).
    pub(super) fn watch_front_app(&mut self) {
        let Some(pid) = apps::frontmost_pid() else { return };
        if pid == self.front_pid {
            return;
        }
        self.front_pid = pid;
        // during a switch, an app of that group becoming active is part of the switch; its
        // first window may still be one that belongs elsewhere
        let ours = self.active.and_then(|id| self.group(id)).is_some_and(|g| g.members.iter().any(|m| m.pid == pid));
        if (self.switch.is_some() && ours) || self.groups.is_empty() || pid == own_pid() {
            return;
        }
        if let Some(w) = windows::ordinary().iter().find(|w| w.pid == pid) {
            self.note_focus(w);
        }
    }

    /// Work out which group is in use from the focused window.
    pub(super) fn note_focus(&mut self, w: &Window) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.has(w.id)) {
            g.used = w.id;
            let id = g.id;
            if self.active != Some(id) {
                self.come_to(Some(id), w.id);
            }
            return;
        }
        let Some(g) = self.active.and_then(|id| self.group_mut(id)) else { return };
        let region = g.layout.region();
        let (x, y) = w.bounds.center();
        if !window::is_small(&w.bounds, &region) {
            self.active = None; // a large window: the user left the group
        } else if region.contains(x, y) && !g.panes.contains(&w.id) {
            g.panes.push(w.id);
        }
    }
}
