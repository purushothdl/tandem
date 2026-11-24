//! Rectangles and axes. The origin is the top left of the main display, y points down.

/// Frames closer than this count as equal: apps round to whole points.
const NEAR: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn other(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// The same, give or take rounding.
    pub fn near(&self, o: &Rect) -> bool {
        self.distance(o) <= NEAR
    }

    /// Largest difference of any component.
    pub fn distance(&self, o: &Rect) -> f64 {
        (self.x - o.x).abs().max((self.y - o.y).abs()).max((self.w - o.w).abs()).max((self.h - o.h).abs())
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Part of the way to another rectangle.
    pub fn lerp(&self, to: &Rect, t: f64) -> Rect {
        // edges, not sizes: neighbours share an edge and must round the same way
        let e = |a: f64, b: f64| (a + (b - a) * t).round();
        let (x, y) = (e(self.x, to.x), e(self.y, to.y));
        Rect { x, y, w: e(self.right(), to.right()) - x, h: e(self.bottom(), to.bottom()) - y }
    }

    /// The edge where the rectangle starts along an axis.
    pub fn lo(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
        }
    }

    /// The edge where it ends.
    pub fn hi(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.right(),
            Axis::Y => self.bottom(),
        }
    }

    pub fn length(&self, axis: Axis) -> f64 {
        self.hi(axis) - self.lo(axis)
    }

    pub fn area(&self) -> f64 {
        self.w * self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Cut in two at `at`.
    pub fn cut(&self, axis: Axis, at: f64) -> (Rect, Rect) {
        match axis {
            Axis::X => (Rect { w: at - self.x, ..*self }, Rect { x: at, w: self.right() - at, ..*self }),
            Axis::Y => (Rect { h: at - self.y, ..*self }, Rect { y: at, h: self.bottom() - at, ..*self }),
        }
    }
}
