//! After a switch: look again a few times, because the chosen window's own activation lands
//! a moment after ours and can undo it. The delays grow, like a retry with backoff.

use std::time::{Duration, Instant};

const DELAYS_MS: [u64; 6] = [70, 120, 220, 350, 500, 700];
/// Failures this early are part of the switch; later ones mean the user moved on.
const GRACE: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct Recheck {
    /// The window that was chosen.
    pub window: u32,
    since: Instant,
    done: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    Check,
    Finished,
}

impl Recheck {
    pub fn new(window: u32, now: Instant) -> Recheck {
        Recheck { window, since: now, done: 0 }
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        now - self.since
    }

    pub fn step(&mut self, now: Instant) -> Step {
        match DELAYS_MS.get(self.done) {
            None => Step::Finished,
            Some(ms) if self.elapsed(now) < Duration::from_millis(*ms) => Step::Wait,
            Some(_) => {
                self.done += 1;
                Step::Check
            }
        }
    }

    /// Should a check that found another window in front end it?
    pub fn give_up(&self) -> bool {
        self.done > GRACE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_checks_then_done() {
        let t0 = Instant::now();
        let mut r = Recheck::new(1, t0);
        assert_eq!(r.step(t0 + Duration::from_millis(60)), Step::Wait);
        for ms in DELAYS_MS {
            assert_eq!(r.step(t0 + Duration::from_millis(ms)), Step::Check);
            assert_eq!(r.step(t0 + Duration::from_millis(ms)), if ms == 700 { Step::Finished } else { Step::Wait });
        }
    }

    #[test]
    fn early_failures_are_forgiven() {
        let t0 = Instant::now();
        let mut r = Recheck::new(1, t0);
        r.step(t0 + Duration::from_millis(70));
        r.step(t0 + Duration::from_millis(120));
        assert!(!r.give_up());
        r.step(t0 + Duration::from_millis(220));
        assert!(r.give_up());
    }
}
