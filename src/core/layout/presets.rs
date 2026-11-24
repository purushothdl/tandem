//! The ready-made shapes offered in the menu.

use crate::core::geometry::Rect;

/// A ready-made shape: one rectangle per window, as parts of a whole.
#[derive(Clone, Debug)]
pub struct Preset {
    pub rects: Vec<Rect>,
    /// The first rectangle is the big one.
    pub main: bool,
}

/// Every shape for this many windows, the plainest first.
pub fn presets(n: usize) -> Vec<Preset> {
    let r = |x: f64, y: f64, w: f64, h: f64| Rect { x, y, w, h };
    let even = |n: usize, i: usize| (i as f64 / n as f64, 1.0 / n as f64);
    let columns = || Preset {
        main: false,
        rects: (0..n)
            .map(|i| {
                let (a, s) = even(n, i);
                r(a, 0.0, s, 1.0)
            })
            .collect(),
    };
    let rows = || Preset {
        main: false,
        rects: (0..n)
            .map(|i| {
                let (a, s) = even(n, i);
                r(0.0, a, 1.0, s)
            })
            .collect(),
    };
    // one big, the rest sharing the other half
    let big = |first: Rect, rest: &dyn Fn(f64, f64) -> Rect| Preset {
        main: true,
        rects: std::iter::once(first)
            .chain((0..n - 1).map(|i| {
                let (a, s) = even(n - 1, i);
                rest(a, s)
            }))
            .collect(),
    };
    let left = || big(r(0.0, 0.0, 0.5, 1.0), &|a, s| r(0.5, a, 0.5, s));
    let right = || big(r(0.5, 0.0, 0.5, 1.0), &|a, s| r(0.0, a, 0.5, s));
    let top = || big(r(0.0, 0.0, 1.0, 0.5), &|a, s| r(a, 0.5, s, 0.5));
    // two rows, filled column by column so neighbours stay neighbours
    let grid = || {
        let upper = n / 2 + n % 2;
        let mut v: Vec<Rect> = Vec::new();
        for i in 0..n {
            let (row, k) = (i % 2, i / 2);
            let (a, s) = even(if row == 0 { upper } else { n - upper }, k);
            v.push(r(a, row as f64 * 0.5, s, 0.5));
        }
        Preset { main: false, rects: v }
    };
    // a half, a quarter, and two eighths
    let halving = || Preset {
        main: true,
        rects: vec![r(0.0, 0.0, 0.5, 1.0), r(0.5, 0.0, 0.5, 0.5), r(0.5, 0.5, 0.25, 0.5), r(0.75, 0.5, 0.25, 0.5)],
    };
    // the same with the eighths above the quarter
    let halving_up = || Preset {
        main: true,
        rects: vec![r(0.0, 0.0, 0.5, 1.0), r(0.5, 0.0, 0.25, 0.5), r(0.75, 0.0, 0.25, 0.5), r(0.5, 0.5, 0.5, 0.5)],
    };
    // a half and four eighths
    let half_and_grid = || Preset {
        main: true,
        rects: vec![
            r(0.0, 0.0, 0.5, 1.0),
            r(0.5, 0.0, 0.25, 0.5),
            r(0.5, 0.5, 0.25, 0.5),
            r(0.75, 0.0, 0.25, 0.5),
            r(0.75, 0.5, 0.25, 0.5),
        ],
    };
    // three quarters and two eighths
    let quarters = || Preset {
        main: true,
        rects: vec![
            r(0.0, 0.0, 0.5, 0.5),
            r(0.0, 0.5, 0.5, 0.5),
            r(0.5, 0.0, 0.5, 0.5),
            r(0.5, 0.5, 0.25, 0.5),
            r(0.75, 0.5, 0.25, 0.5),
        ],
    };
    // left and right exchanged
    let flip = |mut p: Preset| {
        p.rects.iter_mut().for_each(|q| q.x = 1.0 - q.x - q.w);
        p
    };
    match n {
        2 => vec![columns(), rows()],
        3 => vec![columns(), left(), right(), top(), rows()],
        4 => vec![
            grid(),
            left(),
            right(),
            top(),
            halving(),
            flip(halving()),
            halving_up(),
            flip(halving_up()),
            columns(),
        ],
        5 => vec![
            grid(),
            columns(),
            left(),
            right(),
            half_and_grid(),
            flip(half_and_grid()),
            quarters(),
            flip(quarters()),
        ],
        6 => vec![grid(), left(), right(), columns()],
        _ => Vec::new(),
    }
}
