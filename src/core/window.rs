//! Windows as the window server lists them, front to back.

use super::geometry::Rect;

/// Window layers (CGWindowLevel) that matter to us.
mod layer {
    pub const NORMAL: i32 = 0;
    pub const FLOATING: i32 = 3;
    pub const MODAL_PANEL: i32 = 8;
    /// Mission Control's app icons.
    pub const MISSION_CONTROL_ICON: i32 = 17;
    pub const MISSION_CONTROL: i32 = 18;
    pub const DOCK: i32 = 20;
    pub const POPUP_MENU: i32 = 101;
}

/// A window and the app it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRef {
    pub pid: i32,
    pub wid: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub id: u32,
    pub pid: i32,
    pub layer: i32,
    pub alpha: f64,
    pub bounds: Rect,
}

impl Window {
    pub fn reference(&self) -> WindowRef {
        WindowRef { pid: self.pid, wid: self.id }
    }

    /// An open menu or floating panel: presses on it are not for what is underneath.
    pub fn is_menu_or_panel(&self) -> bool {
        matches!(self.layer, layer::FLOATING | layer::MODAL_PANEL | layer::POPUP_MENU)
    }

    pub fn is_mission_control_icon(&self, display: &Rect) -> bool {
        self.layer == layer::MISSION_CONTROL_ICON && self.bounds.w < display.w * 0.5
    }

    /// Electron apps own invisible helper windows.
    pub fn is_visible_normal(&self) -> bool {
        self.layer == layer::NORMAL && self.alpha > 0.05
    }
}

/// The ordinary windows of a list.
pub fn ordinary(all: &[Window]) -> Vec<Window> {
    // the Dock adds normal-layer helpers of its own during Mission Control
    let dock = all.iter().find(|w| w.layer == layer::DOCK).map(|w| w.pid);
    all.iter().filter(|w| w.is_visible_normal() && Some(w.pid) != dock).copied().collect()
}

/// Mission Control and App Exposé cover the display with a Dock window and shrink every
/// window in the list, so positions mean nothing while it is open.
pub fn mission_control_open(all: &[Window], display: &Rect) -> bool {
    all.iter().any(|w| w.layer == layer::MISSION_CONTROL && w.bounds.w >= display.w && w.bounds.h >= display.h)
}

pub fn find(order: &[Window], wid: u32) -> Option<&Window> {
    order.iter().find(|w| w.id == wid)
}

pub fn on_top_at(order: &[Window], x: f64, y: f64) -> Option<&Window> {
    order.iter().find(|w| w.bounds.contains(x, y))
}

/// Small enough to float over a group rather than replace it.
pub fn is_small(w: &Rect, region: &Rect) -> bool {
    w.area() < 0.6 * region.area()
}

/// Share of `order[i]` not hidden behind the windows in front of it, by sampling a grid.
pub fn visible_share(order: &[Window], i: usize) -> f64 {
    const N: usize = 12;
    let b = order[i].bounds;
    let at = |k: usize| (k as f64 + 0.5) / N as f64;
    let seen = (0..N * N)
        .map(|k| (b.x + b.w * at(k / N), b.y + b.h * at(k % N)))
        .filter(|&(x, y)| !order[..i].iter().any(|w| w.bounds.contains(x, y)))
        .count();
    seen as f64 / (N * N) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, pid: i32, layer: i32, x: f64, w: f64) -> Window {
        Window { id, pid, layer, alpha: 1.0, bounds: Rect { x, y: 0.0, w, h: 100.0 } }
    }

    #[test]
    fn half_covered() {
        let order = [window(1, 1, 0, 0.0, 50.0), window(2, 2, 0, 0.0, 100.0)];
        assert_eq!(visible_share(&order, 0), 1.0);
        assert_eq!(visible_share(&order, 1), 0.5);
    }

    #[test]
    fn the_docks_helpers_are_not_windows() {
        let all = [window(1, 9, 20, 0.0, 100.0), window(2, 9, 0, 0.0, 100.0), window(3, 5, 0, 0.0, 100.0)];
        assert_eq!(ordinary(&all).iter().map(|w| w.id).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn mission_control_covers_the_display() {
        let display = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert!(mission_control_open(&[window(1, 9, 18, 0.0, 100.0)], &display));
        assert!(!mission_control_open(&[window(1, 9, 18, 0.0, 40.0)], &display));
    }
}
