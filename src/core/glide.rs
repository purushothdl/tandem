//! Windows easing from where they are to where they belong.

use super::geometry::Rect;
use std::time::{Duration, Instant};

const LENGTH: Duration = Duration::from_millis(300);

pub struct Glide {
    pub from: Vec<Rect>,
    start: Instant,
}

impl Glide {
    pub fn new(from: Vec<Rect>, start: Instant) -> Glide {
        Glide { from, start }
    }

    /// How far along, 0 to 1: slow, fast, slow.
    pub fn progress(&self, now: Instant) -> f64 {
        let t = ((now - self.start).as_secs_f64() / LENGTH.as_secs_f64()).min(1.0);
        if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (2.0 - 2.0 * t).powi(3) / 2.0 }
    }

    pub fn is_done(&self, now: Instant) -> bool {
        now - self.start >= LENGTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_ends_and_never_goes_back() {
        let t0 = Instant::now();
        let g = Glide::new(Vec::new(), t0);
        let at = |ms| g.progress(t0 + Duration::from_millis(ms));
        assert_eq!((at(0), at(300), at(900)), (0.0, 1.0, 1.0));
        assert!((1..300).all(|ms| at(ms) >= at(ms - 1)));
        assert!(!g.is_done(t0 + Duration::from_millis(299)) && g.is_done(t0 + Duration::from_millis(300)));
    }
}
