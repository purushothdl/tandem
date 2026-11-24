//! Telling what a mouse drag means.

use super::geometry::{Axis, Rect};
use super::layout::long_axis;

/// Is a window that went from `from` to `now` being moved by its title bar? A resized window
/// keeps one of its far edges; one whose text is being selected does not move.
pub fn is_title_drag(from: &Rect, now: &Rect) -> bool {
    let (dx, dy) = (now.x - from.x, now.y - from.y);
    let right_stayed = (now.right() - from.right()).abs() < 1.0;
    let bottom_stayed = (now.bottom() - from.bottom()).abs() < 1.0;
    dx.hypot(dy) >= 8.0 && !right_stayed && !(bottom_stayed && dy.abs() > 1.0)
}

/// Which half of a tile a point is in: the axis the tile is split along, and whether it is
/// the half before (left or above).
pub fn half_at(tile: &Rect, x: f64, y: f64) -> (Axis, bool) {
    let axis = long_axis(tile);
    let (cx, cy) = tile.center();
    (axis, if axis == Axis::X { x < cx } else { y < cy })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: Rect = Rect { x: 100.0, y: 100.0, w: 400.0, h: 300.0 };

    #[test]
    fn moving_is_a_drag() {
        assert!(is_title_drag(&FROM, &Rect { x: 130.0, y: 110.0, ..FROM }));
        assert!(!is_title_drag(&FROM, &Rect { x: 103.0, ..FROM }));
    }

    #[test]
    fn resizing_is_not() {
        let left_edge = Rect { x: 140.0, w: 360.0, ..FROM };
        let top_edge = Rect { y: 140.0, h: 260.0, ..FROM };
        assert!(!is_title_drag(&FROM, &left_edge));
        assert!(!is_title_drag(&FROM, &top_edge));
    }

    #[test]
    fn halves_follow_the_longer_side() {
        assert_eq!(half_at(&FROM, 120.0, 390.0), (Axis::X, true));
        assert_eq!(half_at(&Rect { w: 200.0, ..FROM }, 120.0, 390.0), (Axis::Y, false));
    }
}
