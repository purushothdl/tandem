//! How a group divides its region: a binary tree of cuts.
//!
//! A leaf is a window, an inner node cuts its rectangle in two. Any arrangement made of
//! straight cuts is such a tree, and neighbours always share an edge exactly, because the
//! edge is stored once. Only the cuts are stored; frames are computed from them.

mod presets;
#[cfg(test)]
mod tests;

use crate::core::geometry::{Axis, Rect};
pub use presets::{Preset, presets};

/// No window is made smaller than this, whatever it accepts.
pub const MIN_W: f64 = 160.0;
pub const MIN_H: f64 = 100.0;

#[derive(Clone, Debug)]
enum Node {
    Leaf(usize),
    /// `axis`: X puts a left of b (a vertical seam), Y puts a above b.
    Split {
        axis: Axis,
        at: f64,
        a: Box<Node>,
        b: Box<Node>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    /// The axis the seam moves along.
    pub axis: Axis,
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
    /// Minimised windows keep their place in the tree but take no space.
    hidden: Vec<bool>,
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
        let mut l = Layout {
            region: bounds,
            root: build(&all, rects),
            mins: vec![(MIN_W, MIN_H); rects.len()],
            hidden: vec![false; rects.len()],
        };
        l.set_region(region);
        l.snap();
        l
    }

    /// Nearly a half, third or quarter was meant as exactly that.
    fn snap(&mut self) {
        fn walk(n: &mut Node, r: Rect) {
            if let Node::Split { axis, at, a, b } = n {
                let (lo, len) = (r.lo(*axis), r.length(*axis));
                let f = (*at - lo) / len.max(1.0);
                if let Some(c) = [0.5, 1.0 / 3.0, 2.0 / 3.0, 0.25, 0.75].into_iter().find(|c| (f - c).abs() < 0.02) {
                    *at = (lo + c * len).round();
                }
                let (ra, rb) = r.cut(*axis, *at);
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
        fn walk(n: &Node, r: Rect, hidden: &[bool], out: &mut [Rect]) {
            match n {
                Node::Leaf(i) => out[*i] = r,
                Node::Split { axis, at, a, b } => {
                    let (ra, rb) = parts(r, *axis, *at, a, b, hidden);
                    walk(a, ra, hidden, out);
                    walk(b, rb, hidden, out);
                }
            }
        }
        walk(&self.root, self.region, &self.hidden, &mut out);
        out
    }

    /// The seams between windows that are showing.
    pub fn seams(&self) -> Vec<Seam> {
        fn walk(n: &Node, r: Rect, hidden: &[bool], out: &mut Vec<Seam>) {
            if let Node::Split { axis, at, a, b } = n {
                if !gone(a, hidden) && !gone(b, hidden) {
                    out.push(Seam { axis: *axis, at: *at, from: r.lo(axis.other()), to: r.hi(axis.other()) });
                }
                let (ra, rb) = parts(r, *axis, *at, a, b, hidden);
                walk(a, ra, hidden, out);
                walk(b, rb, hidden, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.root, self.region, &self.hidden, &mut out);
        out
    }

    pub fn is_hidden(&self, i: usize) -> bool {
        self.hidden[i]
    }

    /// Window `i` was minimised (its neighbour takes the space) or is back.
    pub fn set_hidden(&mut self, i: usize, hidden: bool) {
        self.hidden[i] = hidden;
        self.fit();
    }

    /// Put seam `k` at `pos`. It stops where a window would get smaller than its minimum;
    /// other seams in the way are pushed. The outer edges never move.
    pub fn drag_seam(&mut self, k: usize, pos: f64) {
        fn walk(n: &mut Node, k: &mut usize, pos: f64, hidden: &[bool]) -> bool {
            let Node::Split { at, a, b, .. } = n else { return false };
            if !gone(a, hidden) && !gone(b, hidden) {
                if *k == 0 {
                    *at = pos.round();
                    return true;
                }
                *k -= 1;
            }
            walk(a, k, pos, hidden) || walk(b, k, pos, hidden)
        }
        let mut k = k;
        walk(&mut self.root, &mut k, pos, &self.hidden);
        self.fit();
    }

    /// Window `i` will not get smaller than this on one axis. True if that was news.
    pub fn learn_min(&mut self, i: usize, axis: Axis, v: f64) -> bool {
        let m = on_axis(&mut self.mins[i], axis);
        if v <= *m {
            return false;
        }
        *m = v;
        self.fit();
        true
    }

    pub fn mins(&self) -> &[(f64, f64)] {
        &self.mins
    }

    /// Forget minimums that only held for a while.
    pub fn restore_mins(&mut self, mins: &[(f64, f64)]) {
        if mins.len() == self.mins.len() {
            self.mins.copy_from_slice(mins);
        }
    }

    /// The same windows in another shape (`rects`: parts of a whole, per window).
    pub fn rearranged(&self, rects: &[Rect]) -> Layout {
        let scaled: Vec<Rect> =
            rects.iter().map(|r| Rect { x: r.x * 6000.0, y: r.y * 6000.0, w: r.w * 6000.0, h: r.h * 6000.0 }).collect();
        let mut l = Layout::from_rects(self.region, &scaled);
        l.mins = self.mins.clone();
        l.hidden = self.hidden.clone();
        l.fit();
        l
    }

    /// Do the minimum sizes fit in the region at all?
    pub fn fits(&self) -> bool {
        need(&self.root, Axis::X, &self.mins, &self.hidden) <= self.region.w
            && need(&self.root, Axis::Y, &self.mins, &self.hidden) <= self.region.h
    }

    /// Move the group to a new region, keeping every proportion.
    pub fn set_region(&mut self, r: Rect) {
        fn walk(n: &mut Node, old: &Rect, new: &Rect) {
            if let Node::Split { axis, at, a, b } = n {
                let len = old.length(*axis).max(1.0);
                let f = (*at - old.lo(*axis)) / len;
                *at = (new.lo(*axis) + f * new.length(*axis)).round();
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
        fn run(n: &Node, s: Axis, hidden: &[bool]) -> f64 {
            match n {
                _ if gone(n, hidden) => 0.0,
                Node::Split { axis, a, b, .. } if *axis == s || gone(a, hidden) || gone(b, hidden) => {
                    run(a, s, hidden) + run(b, s, hidden)
                }
                _ => 1.0,
            }
        }
        fn walk(n: &mut Node, r: Rect, hidden: &[bool]) {
            if let Node::Split { axis, at, a, b } = n {
                if !gone(a, hidden) && !gone(b, hidden) {
                    let (wa, wb) = (run(a, *axis, hidden), run(b, *axis, hidden));
                    *at = (r.lo(*axis) + r.length(*axis) * wa / (wa + wb)).round();
                }
                let (ra, rb) = parts(r, *axis, *at, a, b, hidden);
                walk(a, ra, hidden);
                walk(b, rb, hidden);
            }
        }
        walk(&mut self.root, self.region, &self.hidden);
        self.fit();
    }

    /// A new window takes half of tile `i`, along its longer axis. Returns its index.
    pub fn split(&mut self, i: usize) -> usize {
        let axis = long_axis(&self.frames()[i]);
        self.insert(i, axis, false)
    }

    /// A new window takes half of tile `i`: along which `axis`, and whether
    /// `before` (left or above) or after it. Returns its index.
    pub fn insert(&mut self, i: usize, axis: Axis, before: bool) -> usize {
        fn walk(n: &mut Node, i: usize, cut: (Axis, f64), before: bool, new: usize) -> bool {
            match n {
                Node::Leaf(j) if *j == i => {
                    let (a, b) = if before { (new, i) } else { (i, new) };
                    *n = Node::Split { axis: cut.0, at: cut.1, a: Box::new(Node::Leaf(a)), b: Box::new(Node::Leaf(b)) };
                    true
                }
                Node::Leaf(_) => false,
                Node::Split { a, b, .. } => walk(a, i, cut, before, new) || walk(b, i, cut, before, new),
            }
        }
        let r = self.frames()[i];
        let at = ((r.lo(axis) + r.hi(axis)) / 2.0).round();
        let new = self.mins.len();
        self.mins.push((MIN_W, MIN_H));
        self.hidden.push(false);
        walk(&mut self.root, i, (axis, at), before, new);
        self.fit();
        new
    }

    /// Windows `i` and `j` trade places (the caller swaps its own lists): what is known about
    /// each travels with it, the splits stay. False, with nothing changed, if they do not fit.
    pub fn swap(&mut self, i: usize, j: usize) -> bool {
        let before = self.clone();
        self.mins.swap(i, j);
        self.hidden.swap(i, j);
        self.fit();
        if !self.fits() {
            *self = before;
            return false;
        }
        true
    }

    /// Window `i` is gone: its neighbour takes the space.
    pub fn remove(&mut self, i: usize) {
        fn walk(n: Node, i: usize) -> Option<Node> {
            match n {
                Node::Leaf(j) if j == i => None,
                Node::Leaf(j) => Some(Node::Leaf(if j > i { j - 1 } else { j })),
                Node::Split { axis, at, a, b } => match (walk(*a, i), walk(*b, i)) {
                    (Some(a), Some(b)) => Some(Node::Split { axis, at, a: Box::new(a), b: Box::new(b) }),
                    (a, b) => a.or(b),
                },
            }
        }
        let root = std::mem::replace(&mut self.root, Node::Leaf(0));
        self.root = walk(root, i).unwrap_or(Node::Leaf(0));
        self.mins.remove(i);
        self.hidden.remove(i);
        self.fit();
    }

    /// Pull every seam inside what the minimum sizes allow.
    fn fit(&mut self) {
        fn walk(n: &mut Node, r: Rect, mins: &[(f64, f64)], hidden: &[bool]) {
            if let Node::Split { axis, at, a, b } = n {
                if !gone(a, hidden) && !gone(b, hidden) {
                    let lo = r.lo(*axis) + need(a, *axis, mins, hidden);
                    let hi = r.hi(*axis) - need(b, *axis, mins, hidden);
                    *at = if lo > hi { ((lo + hi) / 2.0).round() } else { at.clamp(lo, hi) };
                }
                let (ra, rb) = parts(r, *axis, *at, a, b, hidden);
                walk(a, ra, mins, hidden);
                walk(b, rb, mins, hidden);
            }
        }
        walk(&mut self.root, self.region, &self.mins, &self.hidden);
    }
}

/// The axis a tile is longer on, which is the one to split it along.
pub fn long_axis(r: &Rect) -> Axis {
    if r.w >= r.h { Axis::X } else { Axis::Y }
}

/// One component of a (width, height).
pub fn on_axis(size: &mut (f64, f64), axis: Axis) -> &mut f64 {
    match axis {
        Axis::X => &mut size.0,
        Axis::Y => &mut size.1,
    }
}

/// Is everything in this subtree minimised?
fn gone(n: &Node, hidden: &[bool]) -> bool {
    match n {
        Node::Leaf(i) => hidden[*i],
        Node::Split { a, b, .. } => gone(a, hidden) && gone(b, hidden),
    }
}

/// The two halves of a split; a axis with nothing showing gives its space away.
fn parts(r: Rect, axis: Axis, at: f64, a: &Node, b: &Node, hidden: &[bool]) -> (Rect, Rect) {
    if gone(a, hidden) || gone(b, hidden) { (r, r) } else { r.cut(axis, at) }
}

/// Smallest size a subtree can take on one axis.
fn need(n: &Node, along: Axis, mins: &[(f64, f64)], hidden: &[bool]) -> f64 {
    match n {
        Node::Leaf(i) if hidden[*i] => 0.0,
        Node::Leaf(i) => match along {
            Axis::X => mins[*i].0,
            Axis::Y => mins[*i].1,
        },
        Node::Split { axis, a, b, .. } => {
            let (x, y) = (need(a, along, mins, hidden), need(b, along, mins, hidden));
            if *axis == along { x + y } else { x.max(y) }
        }
    }
}

/// Find the straight cut that separates these windows with the least overlap, then recurse.
fn build(ids: &[usize], rects: &[Rect]) -> Node {
    if ids.len() == 1 {
        return Node::Leaf(ids[0]);
    }
    struct Cut {
        overlap: f64,
        axis: Axis,
        at: f64,
        a: Vec<usize>,
        b: Vec<usize>,
    }
    let mut best: Option<Cut> = None;
    for axis in [Axis::X, Axis::Y] {
        let mid = |i: &usize| rects[*i].lo(axis) + rects[*i].hi(axis);
        let mut sorted = ids.to_vec();
        sorted.sort_by(|a, b| mid(a).total_cmp(&mid(b)));
        let smallest = ids.iter().map(|i| rects[*i].length(axis)).fold(f64::MAX, f64::min).max(1.0);
        for k in 1..sorted.len() {
            let (a, b) = sorted.split_at(k);
            let a_end = a.iter().map(|i| rects[*i].hi(axis)).fold(f64::MIN, f64::max);
            let b_start = b.iter().map(|i| rects[*i].lo(axis)).fold(f64::MAX, f64::min);
            let overlap = ((a_end - b_start) / smallest).max(0.0);
            if best.as_ref().is_none_or(|x| overlap < x.overlap - 0.02) {
                best = Some(Cut { overlap, axis, at: (a_end + b_start) / 2.0, a: a.to_vec(), b: b.to_vec() });
            }
        }
    }
    let Cut { axis, at, a, b, .. } = best.expect("two or more windows have a cut");
    Node::Split { axis, at, a: Box::new(build(&a, rects)), b: Box::new(build(&b, rects)) }
}
