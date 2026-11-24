//! Pure layout math: a tree of splits, so any arrangement of halves, columns and
//! stacks keeps its shape. No macOS calls in here, so it is unit-tested.

pub const TOL: f64 = 2.0;
pub const MIN_W: f64 = 160.0;
pub const MIN_H: f64 = 100.0;

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

    pub fn near(&self, o: &Rect) -> bool {
        self.distance(o) <= TOL
    }

    /// Largest difference of any component.
    pub fn distance(&self, o: &Rect) -> f64 {
        (self.x - o.x).abs().max((self.y - o.y).abs()).max((self.w - o.w).abs()).max((self.h - o.h).abs())
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn lerp(&self, to: &Rect, t: f64) -> Rect {
        // edges, not sizes: neighbours share an edge and must round the same way
        let e = |a: f64, b: f64| (a + (b - a) * t).round();
        let (x, y) = (e(self.x, to.x), e(self.y, to.y));
        Rect { x, y, w: e(self.right(), to.right()) - x, h: e(self.bottom(), to.bottom()) - y }
    }

    fn lo(&self, side: bool) -> f64 {
        if side { self.x } else { self.y }
    }

    fn hi(&self, side: bool) -> f64 {
        if side { self.right() } else { self.bottom() }
    }

    /// Cut in two at `at`.
    fn cut(&self, side: bool, at: f64) -> (Rect, Rect) {
        if side {
            (Rect { w: at - self.x, ..*self }, Rect { x: at, w: self.right() - at, ..*self })
        } else {
            (Rect { h: at - self.y, ..*self }, Rect { y: at, h: self.bottom() - at, ..*self })
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Leaf(usize),
    /// `side`: a and b are side by side (a vertical seam); otherwise a is above b.
    Split { side: bool, at: f64, a: Box<Node>, b: Box<Node> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    pub side: bool,
    pub at: f64,
    /// Extent along the seam.
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug)]
pub struct Layout {
    region: Rect,
    root: Node,
    /// Smallest (width, height) each window accepts, as far as known.
    mins: Vec<(f64, f64)>,
}

impl Layout {
    /// Recognise the arrangement of these windows and stretch it over `region`.
    pub fn from_rects(region: Rect, rects: &[Rect]) -> Self {
        let x = rects.iter().map(|r| r.x).fold(f64::MAX, f64::min);
        let y = rects.iter().map(|r| r.y).fold(f64::MAX, f64::min);
        let r = rects.iter().map(|r| r.right()).fold(f64::MIN, f64::max);
        let b = rects.iter().map(|r| r.bottom()).fold(f64::MIN, f64::max);
        let bounds = Rect { x, y, w: (r - x).max(1.0), h: (b - y).max(1.0) };
        let all: Vec<usize> = (0..rects.len()).collect();
        let mut l = Layout { region: bounds, root: build(&all, rects), mins: vec![(MIN_W, MIN_H); rects.len()] };
        l.set_region(region);
        l.snap();
        l
    }

    /// Nearly a half, third or quarter was meant as exactly that.
    fn snap(&mut self) {
        fn walk(n: &mut Node, r: Rect) {
            if let Node::Split { side, at, a, b } = n {
                let (lo, len) = (r.lo(*side), r.hi(*side) - r.lo(*side));
                let f = (*at - lo) / len.max(1.0);
                if let Some(c) = [0.5, 1.0 / 3.0, 2.0 / 3.0, 0.25, 0.75].into_iter().find(|c| (f - c).abs() < 0.02) {
                    *at = (lo + c * len).round();
                }
                let (ra, rb) = r.cut(*side, *at);
                walk(a, ra);
                walk(b, rb);
            }
        }
        walk(&mut self.root, self.region);
        self.fit();
    }

    pub fn region(&self) -> Rect {
        self.region
    }

    pub fn frames(&self) -> Vec<Rect> {
        let mut out = vec![Rect::default(); self.mins.len()];
        fn walk(n: &Node, r: Rect, out: &mut [Rect]) {
            match n {
                Node::Leaf(i) => out[*i] = r,
                Node::Split { side, at, a, b } => {
                    let (ra, rb) = r.cut(*side, *at);
                    walk(a, ra, out);
                    walk(b, rb, out);
                }
            }
        }
        walk(&self.root, self.region, &mut out);
        out
    }

    pub fn seams(&self) -> Vec<Seam> {
        fn walk(n: &Node, r: Rect, out: &mut Vec<Seam>) {
            if let Node::Split { side, at, a, b } = n {
                out.push(Seam { side: *side, at: *at, from: r.lo(!side), to: r.hi(!side) });
                let (ra, rb) = r.cut(*side, *at);
                walk(a, ra, out);
                walk(b, rb, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.root, self.region, &mut out);
        out
    }

    /// Put seam `k` at `pos`. It stops where a window would get smaller than its minimum;
    /// other seams in the way are pushed. The outer edges never move.
    pub fn drag_seam(&mut self, k: usize, pos: f64) {
        fn walk(n: &mut Node, k: &mut usize, pos: f64) -> bool {
            let Node::Split { at, a, b, .. } = n else { return false };
            if *k == 0 {
                *at = pos.round();
                return true;
            }
            *k -= 1;
            walk(a, k, pos) || walk(b, k, pos)
        }
        let mut k = k;
        walk(&mut self.root, &mut k, pos);
        self.fit();
    }

    /// Window `i` will not get smaller than this on one axis. True if that was news.
    pub fn learn_min(&mut self, i: usize, width: bool, v: f64) -> bool {
        let m = if width { &mut self.mins[i].0 } else { &mut self.mins[i].1 };
        if v <= *m {
            return false;
        }
        *m = v;
        self.fit();
        true
    }

    /// Do the minimum sizes fit in the region at all?
    pub fn fits(&self) -> bool {
        need(&self.root, true, &self.mins) <= self.region.w && need(&self.root, false, &self.mins) <= self.region.h
    }

    /// Move the group to a new region, keeping every proportion.
    pub fn set_region(&mut self, r: Rect) {
        fn walk(n: &mut Node, old: &Rect, new: &Rect) {
            if let Node::Split { side, at, a, b } = n {
                let len = (old.hi(*side) - old.lo(*side)).max(1.0);
                let f = (*at - old.lo(*side)) / len;
                *at = (new.lo(*side) + f * (new.hi(*side) - new.lo(*side))).round();
                walk(a, old, new);
                walk(b, old, new);
            }
        }
        let old = self.region;
        walk(&mut self.root, &old, &r);
        self.region = r;
        self.fit();
    }

    /// Same share for every window along each run of columns or rows.
    pub fn equalize(&mut self) {
        fn run(n: &Node, s: bool) -> f64 {
            match n {
                Node::Split { side, a, b, .. } if *side == s => run(a, s) + run(b, s),
                _ => 1.0,
            }
        }
        fn walk(n: &mut Node, r: Rect) {
            if let Node::Split { side, at, a, b } = n {
                let (wa, wb) = (run(a, *side), run(b, *side));
                *at = (r.lo(*side) + (r.hi(*side) - r.lo(*side)) * wa / (wa + wb)).round();
                let (ra, rb) = r.cut(*side, *at);
                walk(a, ra);
                walk(b, rb);
            }
        }
        walk(&mut self.root, self.region);
        self.fit();
    }

    /// Window `i` is gone: its neighbour takes the space.
    pub fn remove(&mut self, i: usize) {
        fn walk(n: Node, i: usize) -> Option<Node> {
            match n {
                Node::Leaf(j) if j == i => None,
                Node::Leaf(j) => Some(Node::Leaf(if j > i { j - 1 } else { j })),
                Node::Split { side, at, a, b } => match (walk(*a, i), walk(*b, i)) {
                    (Some(a), Some(b)) => Some(Node::Split { side, at, a: Box::new(a), b: Box::new(b) }),
                    (a, b) => a.or(b),
                },
            }
        }
        let root = std::mem::replace(&mut self.root, Node::Leaf(0));
        self.root = walk(root, i).unwrap_or(Node::Leaf(0));
        self.mins.remove(i);
        self.fit();
    }

    /// Pull every seam inside what the minimum sizes allow.
    fn fit(&mut self) {
        fn walk(n: &mut Node, r: Rect, mins: &[(f64, f64)]) {
            if let Node::Split { side, at, a, b } = n {
                let lo = r.lo(*side) + need(a, *side, mins);
                let hi = r.hi(*side) - need(b, *side, mins);
                *at = if lo > hi { ((lo + hi) / 2.0).round() } else { at.clamp(lo, hi) };
                let (ra, rb) = r.cut(*side, *at);
                walk(a, ra, mins);
                walk(b, rb, mins);
            }
        }
        walk(&mut self.root, self.region, &self.mins);
    }
}

/// Smallest size a subtree can take on one axis.
fn need(n: &Node, width: bool, mins: &[(f64, f64)]) -> f64 {
    match n {
        Node::Leaf(i) => if width { mins[*i].0 } else { mins[*i].1 },
        Node::Split { side, a, b, .. } => {
            let (x, y) = (need(a, width, mins), need(b, width, mins));
            if *side == width { x + y } else { x.max(y) }
        }
    }
}

/// Find the straight cut that separates these windows with the least overlap, then recurse.
fn build(ids: &[usize], rects: &[Rect]) -> Node {
    if ids.len() == 1 {
        return Node::Leaf(ids[0]);
    }
    let mut best: Option<(f64, bool, f64, Vec<usize>, Vec<usize>)> = None;
    for side in [true, false] {
        let mid = |i: &usize| rects[*i].lo(side) + rects[*i].hi(side);
        let mut sorted = ids.to_vec();
        sorted.sort_by(|a, b| mid(a).total_cmp(&mid(b)));
        for k in 1..sorted.len() {
            let (a, b) = sorted.split_at(k);
            let a_end = a.iter().map(|i| rects[*i].hi(side)).fold(f64::MIN, f64::max);
            let b_start = b.iter().map(|i| rects[*i].lo(side)).fold(f64::MAX, f64::min);
            let smallest = ids.iter().map(|i| rects[*i].hi(side) - rects[*i].lo(side)).fold(f64::MAX, f64::min).max(1.0);
            let overlap = ((a_end - b_start) / smallest).max(0.0);
            if best.as_ref().is_none_or(|x| overlap < x.0 - 0.02) {
                best = Some((overlap, side, (a_end + b_start) / 2.0, a.to_vec(), b.to_vec()));
            }
        }
    }
    let (_, side, at, a, b) = best.expect("two or more windows");
    Node::Split { side, at, a: Box::new(build(&a, rects)), b: Box::new(build(&b, rects)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    const R: Rect = Rect { x: 0.0, y: 30.0, w: 1000.0, h: 600.0 };

    fn two() -> Layout {
        Layout::from_rects(R, &[r(0.0, 30.0, 490.0, 600.0), r(510.0, 30.0, 490.0, 600.0)])
    }
    fn half_and_stack() -> Layout {
        Layout::from_rects(R, &[r(0.0, 30.0, 500.0, 600.0), r(500.0, 30.0, 500.0, 300.0), r(500.0, 330.0, 500.0, 300.0)])
    }

    fn covers(l: &Layout) {
        let f = l.frames();
        let area: f64 = f.iter().map(|r| r.w * r.h).sum();
        let g = l.region();
        assert!((area - g.w * g.h).abs() < 1.0, "{f:?}");
        assert!(f.iter().all(|r| r.x >= g.x && r.y >= g.y && r.right() <= g.right() && r.bottom() <= g.bottom()));
    }

    #[test]
    fn two_columns() {
        assert_eq!(two().frames(), vec![r(0.0, 30.0, 500.0, 600.0), r(500.0, 30.0, 500.0, 600.0)]);
    }

    #[test]
    fn half_plus_two_stacked_keeps_its_shape() {
        let l = half_and_stack();
        assert_eq!(l.frames(), vec![r(0.0, 30.0, 500.0, 600.0), r(500.0, 30.0, 500.0, 300.0), r(500.0, 330.0, 500.0, 300.0)]);
        assert_eq!(l.seams().len(), 2);
    }

    #[test]
    fn stack_on_the_left_and_sloppy_placement() {
        let l = Layout::from_rects(R, &[r(8.0, 36.0, 470.0, 280.0), r(3.0, 340.0, 505.0, 285.0), r(495.0, 33.0, 500.0, 590.0)]);
        let f = l.frames();
        assert_eq!(f[0].w, f[1].w);
        assert_eq!((f[2].y, f[2].h), (30.0, 600.0));
        assert_eq!(f[0].w, 500.0); // 0.49 of the width was meant as a half
        covers(&l);
    }

    #[test]
    fn two_by_two_and_rows_and_wide_top() {
        let q = Layout::from_rects(R, &[r(0.0, 0.0, 50.0, 50.0), r(50.0, 0.0, 50.0, 50.0), r(0.0, 50.0, 50.0, 50.0), r(50.0, 50.0, 50.0, 50.0)]);
        assert!(q.frames().iter().all(|f| f.w == 500.0 && f.h == 300.0));
        let rows = Layout::from_rects(R, &[r(0.0, 0.0, 100.0, 30.0), r(0.0, 30.0, 100.0, 70.0)]);
        assert_eq!(rows.frames()[0], r(0.0, 30.0, 1000.0, 180.0));
        let top = Layout::from_rects(R, &[r(0.0, 0.0, 100.0, 50.0), r(0.0, 50.0, 50.0, 50.0), r(50.0, 50.0, 50.0, 50.0)]);
        assert_eq!(top.frames()[0], r(0.0, 30.0, 1000.0, 300.0));
        covers(&q);
        covers(&top);
    }

    #[test]
    fn proportions_survive_tiling_and_a_new_region() {
        let mut l = Layout::from_rects(R, &[r(0.0, 0.0, 300.0, 100.0), r(300.0, 0.0, 700.0, 100.0)]);
        assert_eq!(l.frames()[0].w, 300.0);
        l.set_region(r(0.0, 0.0, 2000.0, 1000.0));
        assert_eq!(l.frames()[0].w, 600.0);
    }

    #[test]
    fn seam_stops_at_both_minimums() {
        let mut l = two();
        l.learn_min(0, true, 300.0);
        l.learn_min(1, true, 400.0);
        l.drag_seam(0, 5000.0);
        assert_eq!(l.frames()[1], r(600.0, 30.0, 400.0, 600.0));
        l.drag_seam(0, -5000.0);
        assert_eq!(l.frames()[0].w, 300.0);
    }

    #[test]
    fn stacked_seam_moves_only_its_own_pair() {
        let mut l = half_and_stack();
        l.drag_seam(1, 200.0);
        let f = l.frames();
        assert_eq!((f[1].h, f[2].h, f[0].h), (170.0, 430.0, 600.0));
        l.learn_min(2, false, 500.0);
        assert_eq!(l.frames()[2].h, 500.0);
        covers(&l);
    }

    #[test]
    fn main_seam_respects_the_widest_minimum_in_the_stack() {
        let mut l = half_and_stack();
        l.learn_min(2, true, 420.0);
        l.drag_seam(0, 900.0);
        assert_eq!(l.frames()[1].w, 420.0);
    }

    #[test]
    fn three_columns_push_and_equalize() {
        let c = |i: f64| r(i * 400.0, 0.0, 400.0, 100.0);
        let mut l = Layout::from_rects(r(0.0, 0.0, 1200.0, 100.0), &[c(0.0), c(1.0), c(2.0)]);
        l.drag_seam(0, 300.0);
        assert_eq!(l.frames().iter().map(|f| f.w).collect::<Vec<_>>(), vec![300.0, 500.0, 400.0]);
        l.drag_seam(0, 1100.0);
        covers(&l);
        assert!(l.frames().iter().all(|f| f.w >= MIN_W));
        l.equalize();
        assert!(l.frames().iter().all(|f| f.w == 400.0));
    }

    #[test]
    fn closing_a_stacked_window_gives_its_space_to_the_neighbour() {
        let mut l = half_and_stack();
        l.remove(1);
        assert_eq!(l.frames(), vec![r(0.0, 30.0, 500.0, 600.0), r(500.0, 30.0, 500.0, 600.0)]);
    }

    #[test]
    fn lerp_keeps_shared_edges_shared() {
        let (a, b) = (r(0.0, 0.0, 333.0, 10.0), r(333.0, 0.0, 667.0, 10.0));
        let (c, d) = (r(0.0, 0.0, 500.0, 10.0), r(500.0, 0.0, 500.0, 10.0));
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert_eq!(a.lerp(&c, t).right(), b.lerp(&d, t).x);
        }
    }
}
