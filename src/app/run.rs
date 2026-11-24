//! The event loop: what runs when.
//!
//! | Source              | Does                                                  |
//! |---------------------|-------------------------------------------------------|
//! | mouse tap           | seam drags, presses on tiles (`tiles`)                |
//! | window notification | closed, minimised (`members`)                         |
//! | 4 ms timer          | hides our outlines the moment the button goes down    |
//! | 16 ms timer         | `frame`: overlays, switches, glides                   |
//! | every 6th frame     | `tick`: which group is in use, strays back in place   |

use super::{App, commands, mission_control, own_pid, try_with};
use crate::core::window;
use crate::platform::element::is_trusted;
use crate::platform::log::log;
use crate::platform::mouse::{self, Event, Verdict};
use crate::platform::{apps, dock, hotkeys, screen, timer, windows};
use crate::ui;
use objc2::MainThreadMarker;
use std::cell::Cell;
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(16);
const PRESS_LOOK: Duration = Duration::from_millis(4);
/// In frames.
const TICK: u32 = 6;
const FRONT_LOOK: u32 = 2;

pub fn start() {
    if dock::restore_after_crash() {
        log("the Dock was still hidden from last time: restored");
    }
    hotkeys::install(&[
        (hotkeys::KEY_T, || commands::report(commands::tile_visible())),
        (hotkeys::KEY_L, || commands::report(commands::next_layout())),
    ]);
    timer::every(FRAME, frame);
    timer::every(PRESS_LOOK, ui::mission_control::drop_on_press);
    let tap = is_trusted(false) && mouse::watch(mouse::Handlers { event: mouse_event, interrupted });
    log(if tap { "started" } else { "started without the mouse: grant Accessibility and reopen" });
}

fn mouse_event(e: Event) -> Verdict {
    let verdict = try_with(|a| a.mouse(e)).unwrap_or(Verdict::Pass);
    if verdict == Verdict::Swallow {
        ui::overlay::refresh(e.x, e.y); // the bar moves with the pointer, not a frame later
    }
    verdict
}

fn interrupted() {
    try_with(|a| {
        a.seam_drag = None;
        a.press = None;
    });
}

fn frame() {
    thread_local! {
        static COUNT: Cell<u32> = const { Cell::new(0) };
    }
    let n = COUNT.get().wrapping_add(1);
    COUNT.set(n);

    // drawing goes before anything that waits for other apps
    ui::mission_control::drop_on_press();
    if let (Some(mtm), Some((x, y))) = (MainThreadMarker::new(), screen::pointer()) {
        ui::overlay::refresh(x, y);
        ui::mission_control::refresh(mtm, x, y);
    }
    try_with(|a| {
        a.watch_mission_control();
        a.recheck_switch();
        if n.is_multiple_of(FRONT_LOOK) {
            a.watch_front_app();
        }
        a.finish_joining();
        a.move_windows();
        if n.is_multiple_of(TICK) {
            a.tick();
        } else if n.is_multiple_of(FRONT_LOOK) && !a.groups.is_empty() {
            // between ticks too, so the bar comes up with the thumbnails
            a.note_mission_control(&windows::all());
        }
    });
    mission_control::follow_bar_pick();
}

impl App {
    fn move_windows(&mut self) {
        for g in &mut self.groups {
            if g.settle_at.take_if(|t| *t <= Instant::now()).is_some() {
                g.place_smoothly(&windows::ordinary(), None);
            }
            g.step();
        }
    }

    /// Ten times a second. Asks mostly the window server, so a hung app cannot stall it.
    fn tick(&mut self) {
        self.learn_minimums();
        let all = windows::all();
        if self.note_mission_control(&all) {
            return;
        }
        if self.mission_control.has_prepared() {
            self.unprepare(); // left without choosing
            return;
        }
        let order = window::ordinary(&all);
        let front = apps::frontmost_pid().filter(|p| *p != own_pid());
        if let Some(w) = front.and_then(|pid| order.iter().find(|w| w.pid == pid)) {
            self.last_focused = Some(w.reference());
            if self.switch.is_none() {
                self.note_focus(w);
            }
        }
        if mouse::left_down() || self.seam_drag.is_some() || !self.mission_control.calm() {
            return;
        }
        self.groups.iter_mut().for_each(|g| g.correct(&order));
        if self.switch.is_none()
            && let Some(g) = self.active.and_then(|id| self.group_mut(id))
        {
            g.remember(&order);
        }
    }
}
