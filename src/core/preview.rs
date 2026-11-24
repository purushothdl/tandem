//! The small picture of a group in the Mission Control bar.

use super::geometry::Rect;
use super::layout::presets;

/// A layout with big tiles for this many apps, so that every icon has room: two halves,
/// a half and two quarters, or a grid. Of those, the one closest to the real one, each
/// app in the place nearest to where it really is.
pub fn even(tiles: Vec<(Rect, i32)>) -> Vec<(Rect, i32)> {
    if tiles.len() < 2 {
        return tiles;
    }
    let all = presets(tiles.len());
    let choices = match tiles.len() {
        2 => 0..2,
        3 => 1..4, // the half on the left, on the right, on top
        _ => 0..1,
    };
    let best = choices.filter_map(|k| all.get(k)).map(|p| assign(&tiles, &p.rects)).min_by(|a, b| a.0.total_cmp(&b.0));
    best.map_or(tiles, |b| b.1)
}

/// Give each tile the nearest of these places still free, and how far they moved in all.
fn assign(tiles: &[(Rect, i32)], places: &[Rect]) -> (f64, Vec<(Rect, i32)>) {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, (t, _)) in tiles.iter().enumerate() {
        for (k, r) in places.iter().enumerate() {
            let ((x, y), (u, v)) = (t.center(), r.center());
            // a little for the size too: the big app gets the big place
            pairs.push(((x - u).hypot(y - v) + 0.3 * (t.area() - r.area()).abs(), i, k));
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = tiles.to_vec();
    let (mut total, mut used, mut done) = (0.0, vec![false; places.len()], vec![false; tiles.len()]);
    for (d, i, k) in pairs {
        if !done[i] && !used[k] {
            (done[i], used[k]) = (true, true);
            out[i].0 = places[k];
            total += d;
        }
    }
    (total, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_have_big_tiles() {
        let col = |i: f64, n: f64| Rect { x: i / n, y: 0.0, w: 1.0 / n, h: 1.0 };
        for n in 2..=6usize {
            let out = even((0..n).map(|i| (col(i as f64, n as f64), i as i32)).collect());
            let mut areas: Vec<f64> = out.iter().map(|t| t.0.w * t.0.h).collect();
            areas.sort_by(f64::total_cmp);
            assert!((areas.iter().sum::<f64>() - 1.0).abs() < 1e-6);
            assert!(areas[0] >= [0.5, 0.25, 0.25, 0.16, 0.16][n - 2], "{n}: {areas:?}");
            if n == 3 {
                assert!((areas[2] - 0.5).abs() < 1e-6);
            }
        }
    }
}
