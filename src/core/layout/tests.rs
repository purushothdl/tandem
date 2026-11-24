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
    assert_eq!(
        l.frames(),
        vec![r(0.0, 30.0, 500.0, 600.0), r(500.0, 30.0, 500.0, 300.0), r(500.0, 330.0, 500.0, 300.0)]
    );
    assert_eq!(l.seams().len(), 2);
}

#[test]
fn stack_on_the_left_and_sloppy_placement() {
    let l =
        Layout::from_rects(R, &[r(8.0, 36.0, 470.0, 280.0), r(3.0, 340.0, 505.0, 285.0), r(495.0, 33.0, 500.0, 590.0)]);
    let f = l.frames();
    assert_eq!(f[0].w, f[1].w);
    assert_eq!((f[2].y, f[2].h), (30.0, 600.0));
    assert_eq!(f[0].w, 500.0); // 0.49 of the width was meant as a half
    covers(&l);
}

#[test]
fn two_by_two_and_rows_and_wide_top() {
    let q = Layout::from_rects(
        R,
        &[r(0.0, 0.0, 50.0, 50.0), r(50.0, 0.0, 50.0, 50.0), r(0.0, 50.0, 50.0, 50.0), r(50.0, 50.0, 50.0, 50.0)],
    );
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
    l.learn_min(0, Axis::X, 300.0);
    l.learn_min(1, Axis::X, 400.0);
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
    l.learn_min(2, Axis::Y, 500.0);
    assert_eq!(l.frames()[2].h, 500.0);
    covers(&l);
}

#[test]
fn main_seam_respects_the_widest_minimum_in_the_stack() {
    let mut l = half_and_stack();
    l.learn_min(2, Axis::X, 420.0);
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
fn adding_splits_only_that_tile_along_its_longer_side() {
    let mut l = two(); // both tiles are taller than wide
    assert_eq!(l.split(1), 2);
    let f = l.frames();
    assert_eq!(f[0], r(0.0, 30.0, 500.0, 600.0));
    assert_eq!(f[1], r(500.0, 30.0, 500.0, 300.0));
    assert_eq!(f[2], r(500.0, 330.0, 500.0, 300.0));
    l.split(2); // wider than tall now
    assert_eq!(l.frames()[3], r(750.0, 330.0, 250.0, 300.0));
    covers(&l);
    l.remove(3);
    l.remove(2);
    assert_eq!(l.frames(), two().frames());
}

#[test]
fn adding_where_it_cannot_fit_is_noticed() {
    let mut l = two();
    l.learn_min(1, Axis::Y, 560.0);
    l.split(1);
    assert!(!l.fits());
}

#[test]
fn minimising_gives_the_space_away_and_restoring_returns_it() {
    let mut l = half_and_stack();
    l.drag_seam(1, 200.0);
    let before = l.frames();
    l.set_hidden(1, true);
    assert_eq!(l.frames()[2], r(500.0, 30.0, 500.0, 600.0));
    assert_eq!(l.seams().len(), 1);
    l.set_hidden(2, true);
    assert_eq!(l.frames()[0], R);
    assert!(l.seams().is_empty());
    l.set_hidden(1, false);
    l.set_hidden(2, false);
    assert_eq!(l.frames(), before);
    // the seam that is left is the one that moves
    l.set_hidden(0, true);
    l.drag_seam(0, 400.0);
    assert_eq!(l.frames()[1], r(0.0, 30.0, 1000.0, 370.0));
}

#[test]
fn swapping_keeps_the_splits_but_respects_minimums() {
    let mut l = half_and_stack();
    let before = l.frames();
    assert!(l.swap(0, 1));
    assert_eq!(l.frames(), before);
    // a window that needs 420 points of height moves the stack's seam
    l.learn_min(0, Axis::Y, 420.0);
    assert!(l.swap(0, 1));
    assert_eq!(l.frames()[1].h, 420.0);
    covers(&l);
    // and one that can never fit in the stack stays where it is
    l.swap(0, 1);
    l.learn_min(0, Axis::Y, 590.0);
    let before = l.frames();
    assert!(!l.swap(0, 1));
    assert_eq!(l.frames(), before);
}

#[test]
fn inserting_on_each_edge() {
    for (side, before, want) in [
        (Axis::X, true, r(500.0, 30.0, 250.0, 600.0)),
        (Axis::X, false, r(750.0, 30.0, 250.0, 600.0)),
        (Axis::Y, true, r(500.0, 30.0, 500.0, 300.0)),
        (Axis::Y, false, r(500.0, 330.0, 500.0, 300.0)),
    ] {
        let mut l = two();
        let i = l.insert(1, side, before);
        assert_eq!(l.frames()[i], want);
        assert_eq!(l.frames()[0], r(0.0, 30.0, 500.0, 600.0));
        covers(&l);
    }
}

#[test]
fn every_preset_fills_the_region_as_drawn() {
    let region = r(0.0, 30.0, 1920.0, 984.0);
    for n in 2..=6 {
        let c: Vec<Rect> = (0..n).map(|i| r(i as f64 * 100.0, 0.0, 100.0, 100.0)).collect();
        let l = Layout::from_rects(region, &c);
        for p in presets(n) {
            assert_eq!(p.rects.len(), n);
            let m = l.rearranged(&p.rects);
            covers(&m);
            for (f, w) in m.frames().iter().zip(&p.rects) {
                let want = r(w.x * 1920.0, 30.0 + w.y * 984.0, w.w * 1920.0, w.h * 984.0);
                assert!(f.distance(&want) <= 2.0, "{n}: {f:?} {want:?}");
            }
        }
    }
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
