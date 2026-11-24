//! The groups, and the only code that may change them.
//!
//! The layout is the truth. It changes when the user drags a seam, uses the menu or drops a
//! window, or when an app reports a minimum size. A window found anywhere else is put back;
//! it never reshapes the group.
//!
//! Everything here runs on the main thread, in short callbacks from timers, the mouse and
//! window notifications (`run`).

pub mod commands;
mod group;
mod members;
mod mission_control;
pub mod run;
mod seams;
mod stacking;
mod switching;
mod tiles;
pub mod views;
mod writer;

use crate::core::window::WindowRef;
use crate::platform::dock;
use crate::platform::element::Observer;
use group::Group;
pub use mission_control::press_bar_entry;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

pub type GroupId = u32;

pub const MAX_TILES: usize = 6;

/// One of our dialogs is open: what is behind it must not react.
pub static DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct App {
    groups: Vec<Group>,
    ids_given: GroupId,
    /// The group being worked in, also while using a pane over it.
    active: Option<GroupId>,
    /// The front window of the frontmost other app.
    last_focused: Option<WindowRef>,
    front_pid: i32,
    /// One per app, for its windows being closed or minimised.
    observers: HashMap<i32, Observer>,
    /// How the Dock was, while a group is full screen.
    dock: Option<dock::Saved>,

    seam_drag: Option<seams::SeamDrag>,
    press: Option<tiles::Press>,
    joining: Option<tiles::Joining>,
    switch: Option<crate::core::recheck::Recheck>,
    mission_control: mission_control::MissionControl,
}

thread_local! {
    static APP: RefCell<App> = RefCell::default();
}

fn with<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

/// For callbacks, which can arrive while another one waits for an app to answer.
fn try_with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().map(|mut a| f(&mut a)))
}

impl App {
    fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    fn group_of(&self, wid: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.has(wid))
    }

    /// The group in use, or the nearest thing to it.
    fn target(&self) -> Option<GroupId> {
        let focused = self.last_focused.and_then(|w| self.group_of(w.wid));
        focused.or(self.groups.first()).map(|g| g.id)
    }

    /// The Dock comes back once no group is full screen.
    fn restore_dock_if_idle(&mut self) {
        if !self.groups.iter().any(|g| g.full)
            && let Some(saved) = self.dock.take()
        {
            dock::restore(&saved);
        }
    }
}

fn own_pid() -> i32 {
    std::process::id() as i32
}
