//! When the bar of groups may be up. Thumbnails shrink while Mission Control opens and grow
//! as it closes; between the two they rest.

use std::time::Duration;

/// Thumbnails must have been still this long.
const STILL: Duration = Duration::from_millis(60);
/// More than the small bounce at the end of opening.
const GROWING: f64 = 1.06;

#[derive(Debug, Default)]
pub struct BarPhase {
    /// The smallest the thumbnails have been, together, since they last rested.
    smallest: Option<f64>,
    /// The bar sits where the resting thumbnails leave room.
    placed: bool,
    /// Not opening for the first time any more: the place from last time may be wrong.
    disturbed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Hide,
    /// Work out where it goes.
    Place,
    /// Where it went last time for the same windows, while they are still moving.
    Guess,
    Stay,
}

impl BarPhase {
    /// `area`: of all thumbnails. `calm`: how long none has moved. `up`: the bar is showing.
    pub fn step(&mut self, area: f64, calm: Duration, up: bool) -> Decision {
        let still = calm >= STILL;
        let smallest = if still { area } else { self.smallest.map_or(area, |s| s.min(area)) };
        self.smallest = Some(smallest);
        if area > smallest * GROWING || (self.placed && !still) {
            self.disturbed = true;
            self.placed = false;
            return Decision::Hide;
        }
        match (still && !self.placed, up) {
            (true, _) => Decision::Place,
            (false, true) => Decision::Stay,
            (false, false) if !self.disturbed => Decision::Guess,
            (false, false) => Decision::Hide,
        }
    }

    /// It found its place. True if that is worth remembering for next time.
    pub fn place(&mut self) -> bool {
        self.placed = true;
        !std::mem::replace(&mut self.disturbed, true)
    }

    /// It was taken down.
    pub fn unplace(&mut self) {
        self.placed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVING: Duration = Duration::ZERO;
    const RESTING: Duration = Duration::from_millis(100);

    #[test]
    fn opens_rests_and_closes() {
        let mut p = BarPhase::default();
        assert_eq!(p.step(100.0, MOVING, false), Decision::Guess);
        assert_eq!(p.step(50.0, MOVING, true), Decision::Stay);
        assert_eq!(p.step(49.0, RESTING, true), Decision::Place);
        assert!(p.place());
        assert_eq!(p.step(49.0, RESTING, true), Decision::Stay);
        assert_eq!(p.step(55.0, MOVING, true), Decision::Hide);
        assert_eq!(p.step(70.0, MOVING, false), Decision::Hide);
    }

    #[test]
    fn the_bounce_at_the_end_does_not_hide_it() {
        let mut p = BarPhase::default();
        p.step(50.0, MOVING, true);
        assert_eq!(p.step(51.0, MOVING, true), Decision::Stay);
    }

    #[test]
    fn a_rearrangement_is_placed_anew_but_not_remembered() {
        let mut p = BarPhase::default();
        p.step(50.0, RESTING, false);
        p.place();
        assert_eq!(p.step(45.0, MOVING, true), Decision::Hide);
        assert_eq!(p.step(45.0, MOVING, false), Decision::Hide);
        assert_eq!(p.step(45.0, RESTING, false), Decision::Place);
        assert!(!p.place());
    }
}
