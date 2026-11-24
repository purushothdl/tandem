//! Where the bar of groups goes in Mission Control. It must not cover a thumbnail, because
//! the click that picks a group also lands on whatever is underneath.

use super::geometry::Rect;

/// Between the bar and any thumbnail.
const CLEAR: f64 = 20.0;
/// The same for the single line, which has less room.
const ABOVE: f64 = 8.0;
/// The closed strip of desktops at the top of the screen.
const DESKTOPS: f64 = 40.0;
/// Between the bar and the sides of the screen.
const SIDE: f64 = 24.0;
/// The bar does not float further than this above the Dock.
const LOW: f64 = 36.0;

/// Width and height.
pub type Size = (f64, f64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// The big bar, rather than the single line.
    pub big: bool,
    /// Top left.
    pub at: (f64, f64),
}

/// The big bar at the top if there is room, then at the bottom; then the single line.
/// `floor`: the top of the Dock.
pub fn spot(big: Size, line: Size, screen_w: f64, floor: f64, taken: &[Rect]) -> Option<Spot> {
    let top = taken.iter().map(|t| t.y).fold(f64::INFINITY, f64::min);
    let (w, h) = big;
    if let Some(p) = high_spot(w, h, screen_w, top, taken).or_else(|| low_spot(w, h, screen_w, floor, taken)) {
        return Some(Spot { big: true, at: p });
    }
    let (w, h) = line;
    let y = top - ABOVE - h;
    if y >= DESKTOPS && y.is_finite() && w <= screen_w {
        return Some(Spot { big: false, at: ((screen_w - w) / 2.0, y) });
    }
    low_spot(w, h, screen_w, floor, taken).map(|p| Spot { big: false, at: p })
}

/// Places from left to right worth trying, nearest the middle first.
fn columns(w: f64, screen_w: f64, taken: &[Rect]) -> Vec<f64> {
    let mid = (screen_w - w) / 2.0;
    let mut xs = vec![mid, SIDE, screen_w - SIDE - w];
    // just beside each obstacle
    xs.extend(taken.iter().flat_map(|t| [t.x - CLEAR - w, t.right() + CLEAR]));
    xs.retain(|x| *x >= SIDE && x + w <= screen_w - SIDE);
    xs.sort_by(|a, b| (a - mid).abs().total_cmp(&(b - mid).abs()));
    xs
}

fn in_column(x: f64, w: f64, taken: &[Rect]) -> impl Iterator<Item = &Rect> {
    taken.iter().filter(move |t| t.x < x + w + CLEAR - 0.5 && t.right() > x - CLEAR + 0.5)
}

/// Level with the topmost thumbnails, or higher if those below need it. Thumbnails move
/// down with the strip of desktops when it opens, and so does this.
fn high_spot(w: f64, h: f64, screen_w: f64, top: f64, taken: &[Rect]) -> Option<(f64, f64)> {
    columns(w, screen_w, taken).into_iter().find_map(|x| {
        let below = in_column(x, w, taken).map(|t| t.y).fold(f64::INFINITY, f64::min);
        let y = top.min(below - CLEAR - h);
        (y.is_finite() && y >= DESKTOPS + 10.0).then_some((x, y))
    })
}

/// Along the bottom.
fn low_spot(w: f64, h: f64, screen_w: f64, floor: f64, taken: &[Rect]) -> Option<(f64, f64)> {
    columns(w, screen_w, taken).into_iter().find_map(|x| {
        let low = in_column(x, w, taken).filter(|t| t.y < floor).map(|t| t.bottom()).fold(0.0, f64::max);
        let room = floor - low;
        (room >= h + 2.0 * CLEAR).then(|| (x, floor - h - ((room - h) / 2.0).min(LOW)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: f64 = 68.0;
    const LINE: f64 = 34.0;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    // measured on a real 1920x1080 display: two groups, the middle taken down to the Dock
    fn measured() -> Vec<Rect> {
        vec![
            r(1372.0, 89.0, 530.0, 543.0),
            r(136.0, 161.0, 530.0, 544.0),
            r(702.0, 435.0, 634.0, 543.0),
            r(18.0, 89.0, 427.0, 543.0),
            r(969.0, 923.0, 100.0, 109.0),
            r(283.0, 650.0, 118.0, 109.0),
            r(1587.0, 577.0, 100.0, 109.0),
        ]
    }

    fn clear(me: Rect) -> bool {
        measured().iter().all(|t| me.right() <= t.x || me.x >= t.right() || me.y >= t.bottom() || me.bottom() <= t.y)
    }

    #[test]
    fn big_at_the_top_between_thumbnails() {
        let Spot { big, at: (x, y) } = spot((560.0, BIG), (500.0, LINE), 1920.0, 1010.0, &measured()).expect("room");
        assert!(big && y > DESKTOPS && y < 200.0 && clear(r(x, y, 560.0, BIG)));
    }

    #[test]
    fn big_at_the_bottom_if_too_wide_for_that() {
        let Spot { big, at: (x, y) } = spot((640.0, BIG), (500.0, LINE), 1920.0, 1010.0, &measured()).expect("room");
        assert!(big && clear(r(x, y, 640.0, BIG)));
    }

    #[test]
    fn a_line_when_there_is_only_that_much() {
        let full = [r(20.0, 89.0, 1880.0, 880.0)];
        let Spot { big, at: (_, y) } = spot((560.0, BIG), (500.0, LINE), 1920.0, 1010.0, &full).expect("room");
        assert!(!big && y >= DESKTOPS && y + LINE < 89.0);
    }

    #[test]
    fn none_when_full() {
        assert!(spot((560.0, BIG), (500.0, LINE), 1920.0, 1010.0, &[r(0.0, 60.0, 1920.0, 930.0)]).is_none());
    }
}
