//! What the menu, the dialogs and the shortcuts can ask for.

use super::{App, GroupId, MAX_TILES, own_pid, with};
use crate::core::layout::{Layout, presets};
use crate::core::window::{self, Window, WindowRef};
use crate::platform::element::{find_window, is_trusted};
use crate::platform::log::log;
use crate::platform::{corners, dock, focus, screen, timer, windows};
use std::time::Duration;

/// What was done, or what to tell the user.
pub type Outcome = Result<String, String>;

pub fn report(o: Outcome) {
    log(&o.unwrap_or_else(|e| e));
}

const NO_PERMISSION: &str =
    "Needs Accessibility permission: add wingroup in System Settings > Privacy & Security > Accessibility, then reopen";

/// Smaller windows are palettes and the like.
pub const SMALLEST_WINDOW: f64 = 200.0;
/// Layouts closer than this look the same.
const SAME: f64 = 24.0;

/// The windows Tile would take: on the main display and at least half in view.
pub fn visible_picks() -> Vec<Window> {
    let region = screen::visible();
    let mut order = windows::ordinary();
    order.retain(|w| {
        let (x, y) = w.bounds.center();
        w.pid != own_pid() && w.bounds.w >= SMALLEST_WINDOW && w.bounds.h >= SMALLEST_WINDOW && region.contains(x, y)
    });
    let in_view = (0..order.len()).filter(|&i| window::visible_share(&order, i) >= 0.5).map(|i| order[i]);
    let standard = in_view.filter(|w| find_window(w.pid, w.id).is_some_and(|e| e.is_standard_window()));
    standard.take(2 * MAX_TILES).collect()
}

/// Group the windows you can see, in the order and proportions you arranged them. Windows
/// already in a group stay there.
pub fn tile_visible() -> Outcome {
    if !is_trusted(false) {
        return Err(NO_PERMISSION.into());
    }
    let seen = visible_picks();
    with(|a| {
        let free = seen.iter().filter(|w| a.group_of(w.id).is_none());
        let picks: Vec<WindowRef> = free.take(MAX_TILES).map(Window::reference).collect();
        match picks.len() {
            0 | 1 if picks.len() < seen.len() => {
                Err("Need two ungrouped windows in view (ungroup this one to change it)".into())
            }
            0 | 1 => Err("Place at least two windows side by side first".into()),
            _ => a.make_group(&picks, screen::visible()),
        }
    })
}

/// Tile exactly these windows.
pub fn tile_chosen(picks: &[WindowRef]) -> Outcome {
    if !is_trusted(false) {
        return Err(NO_PERMISSION.into());
    }
    let (done, id) = with(|a| (a.make_group(picks, screen::visible()), a.active));
    // they may have been hidden behind other windows
    if let (Ok(_), Some(id)) = (&done, id) {
        switch_to(id)?;
    }
    done
}

pub fn add_windows(id: GroupId, picks: &[WindowRef]) -> Outcome {
    let done = with(|a| a.add_to(id, picks))?;
    switch_to(id)?;
    Ok(done)
}

/// Back to the size it had before, and behind the group it no longer belongs to.
pub fn remove_window(wid: u32) -> Outcome {
    with(|a| {
        let Some((gi, mi)) = a.find_member(|m| m.wid == wid) else {
            return Err("No such window in a group".into());
        };
        let g = &mut a.groups[gi];
        let id = g.id;
        let (frames, home) = (g.members[mi].frames.clone(), g.members[mi].home);
        let others = (g.members.iter().enumerate()).filter(|(i, _)| *i != mi && g.is_shown(*i));
        let next = others.max_by_key(|(_, m)| m.wid == g.used).map(|(_, m)| (m.reference(), m.element.clone()));
        g.forget(wid);
        a.drop_member(gi, mi);
        if let Some((w, element)) = next {
            focus::activate(w.pid, &element);
            a.come_to(a.group(id).map(|g| g.id), w.wid);
        }
        if let Some(home) = home {
            // once the group is in front of it
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(350));
                let _ = frames.send(home);
            });
        }
        Ok("Removed".into())
    })
}

pub fn set_layout(id: GroupId, k: usize) -> Outcome {
    with(|a| {
        a.seam_drag = None;
        let g = a.group_mut(id).ok_or("No group")?;
        g.set_preset(k, &windows::ordinary()).then_some(()).ok_or("No room for that layout".to_string())
    })?;
    switch_to(id)?;
    Ok("Layout changed".into())
}

/// The next layout that fits and looks different.
pub fn next_layout() -> Outcome {
    with(|a| {
        a.seam_drag = None;
        let g = a.target().and_then(|id| a.group_mut(id)).ok_or("No group")?;
        let now = g.layout.frames();
        let same = |l: &Layout| l.frames().iter().zip(&now).all(|(a, b)| a.distance(b) < SAME);
        let all = presets(g.members.len());
        let next = (1..=all.len()).map(|step| (g.preset + step) % all.len()).find(|&k| {
            let l = g.arranged(&all[k]);
            l.fits() && !same(&l)
        });
        match next {
            Some(k) if g.set_preset(k, &windows::ordinary()) => Ok("Next layout".into()),
            _ => Err("No other layout fits".into()),
        }
    })
}

pub fn ungroup(id: GroupId) -> Outcome {
    with(|a| {
        let i = a.groups.iter().position(|g| g.id == id).ok_or("No group")?;
        a.seam_drag = None;
        a.groups.remove(i);
        a.restore_dock_if_idle();
        Ok("Ungrouped".into())
    })
}

pub fn ungroup_all() -> Outcome {
    with(|a| {
        a.seam_drag = None;
        a.groups.clear();
        a.restore_dock_if_idle();
        Ok("Ungrouped all".into())
    })
}

/// Bring a group to the front and give its last used window the keyboard.
pub fn switch_to(id: GroupId) -> Outcome {
    with(|a| {
        let m = a.group(id).ok_or("No group")?.main_member();
        let (w, element) = (m.reference(), m.element.clone());
        focus::activate(w.pid, &element);
        a.come_to(Some(id), w.wid);
        Ok("Switched".into())
    })
}

pub fn rename(id: GroupId, name: &str) -> Outcome {
    with(|a| match a.group_mut(id) {
        Some(g) if !name.trim().is_empty() => {
            g.name = name.trim().to_string();
            g.named = true;
            Ok(format!("Named “{}”", g.name))
        }
        _ => Ok("Name unchanged".into()),
    })
}

/// Full screen is for all groups at once: the Dock it hides is system-wide.
pub fn toggle_full() -> Outcome {
    with(|a| {
        let on = !a.groups.iter().any(|g| g.full);
        let order = windows::ordinary();
        if on && a.dock.is_none() {
            a.dock = Some(dock::hide());
        }
        for g in &mut a.groups {
            g.set_full(on);
            g.place_smoothly(&order, None);
        }
        a.seam_drag = None;
        if on {
            timer::after(Duration::from_millis(900), use_freed_space);
        } else {
            a.restore_dock_if_idle();
        }
        Ok(format!("Full screen {}", if on { "on" } else { "off" }))
    })
}

/// Once the Dock has finished hiding.
fn use_freed_space() {
    with(|a: &mut App| {
        for g in a.groups.iter_mut().filter(|g| g.full) {
            g.layout.set_region(screen::below_menu_bar());
            g.expected.clear();
            g.place();
        }
    });
}

/// Apps pick this up when they are next opened.
pub fn set_corners(radius: u32) -> Outcome {
    if corners::set_radius(radius) {
        Ok(format!("Corners {radius}"))
    } else {
        Err("No change: macOS refused it".into())
    }
}

/// Leave the system as it was.
pub fn shutdown() {
    with(|a| {
        a.groups.iter_mut().for_each(|g| g.full = false);
        a.restore_dock_if_idle();
    });
}
