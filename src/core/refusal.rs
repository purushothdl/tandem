//! Telling an app's real minimum size from an app that is only slow.
//!
//! Both look the same: we ask for 300 points and read back 600. A real minimum stays; a busy
//! app catches up. So a refusal only counts once it has lasted.

use super::geometry::Axis;
use std::time::{Duration, Instant};

/// Two different requests, same answer: good enough while a seam is being dragged.
const REPEATED: Duration = Duration::from_millis(90);
/// Still refusing with nothing else going on: worth remembering.
const LASTING: Duration = Duration::from_millis(500);
/// Slack for apps that round.
const SLACK: f64 = 2.0;

/// A size an app would not go below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minimum {
    pub size: f64,
    /// Otherwise it must not be remembered beyond the drag going on.
    pub sure: bool,
}

#[derive(Clone, Copy)]
struct Refusal {
    asked: f64,
    got: f64,
    since: Instant,
}

/// What one window has been refusing.
#[derive(Default)]
pub struct Refusals {
    x: Option<Refusal>,
    y: Option<Refusal>,
}

impl Refusals {
    /// We asked for a size and read one back. `settled`: we asked many times, for this alone.
    pub fn observe(&mut self, axis: Axis, asked: f64, got: f64, settled: bool, now: Instant) -> Option<Minimum> {
        let slot = match axis {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
        };
        if got <= asked + SLACK {
            *slot = None;
            return None;
        }
        let before = slot.filter(|r| r.got == got);
        let lasted = before.map_or(Duration::ZERO, |r| now - r.since);
        let asked_differently = before.is_some_and(|r| r.asked != asked);
        *slot = Some(Refusal { asked, got, since: before.map_or(now, |r| r.since) });

        let sure = settled && lasted > LASTING;
        (sure || (asked_differently && lasted > REPEATED)).then(|| Minimum { size: got.ceil(), sure })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_slow_app_is_not_a_minimum() {
        let (mut r, t0) = (Refusals::default(), Instant::now());
        assert_eq!(r.observe(Axis::X, 300.0, 600.0, true, t0), None);
        assert_eq!(r.observe(Axis::X, 300.0, 600.0, true, t0 + ms(400)), None);
        // it caught up, so the clock starts again
        assert_eq!(r.observe(Axis::X, 300.0, 300.0, true, t0 + ms(450)), None);
        assert_eq!(r.observe(Axis::X, 300.0, 600.0, true, t0 + ms(600)), None);
    }

    #[test]
    fn a_lasting_refusal_is_sure() {
        let (mut r, t0) = (Refusals::default(), Instant::now());
        r.observe(Axis::Y, 300.0, 599.5, true, t0);
        let m = r.observe(Axis::Y, 300.0, 599.5, true, t0 + ms(501));
        assert_eq!(m, Some(Minimum { size: 600.0, sure: true }));
    }

    #[test]
    fn during_a_drag_it_only_counts_for_the_drag() {
        let (mut r, t0) = (Refusals::default(), Instant::now());
        r.observe(Axis::X, 500.0, 600.0, false, t0);
        assert_eq!(r.observe(Axis::X, 490.0, 600.0, false, t0 + ms(50)), None);
        let m = r.observe(Axis::X, 480.0, 600.0, false, t0 + ms(100));
        assert_eq!(m, Some(Minimum { size: 600.0, sure: false }));
    }

    #[test]
    fn axes_are_separate() {
        let (mut r, t0) = (Refusals::default(), Instant::now());
        r.observe(Axis::X, 300.0, 600.0, true, t0);
        assert_eq!(r.observe(Axis::Y, 300.0, 600.0, true, t0 + ms(600)), None);
    }
}
