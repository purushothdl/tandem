//! A group chosen from the bar in Mission Control. The press also closes Mission Control,
//! which can hand the front back to the window it was opened from; then switch once more.

use std::time::{Duration, Instant};

/// Watch this long after Mission Control has closed.
const WATCH: Duration = Duration::from_millis(350);
const RETRY: Duration = Duration::from_millis(150);
/// It never closed: forget about it.
const TIMEOUT: Duration = Duration::from_millis(1800);

#[derive(Clone, Copy, Debug)]
pub struct CardPick {
    pub group: u32,
    start: Instant,
    state: State,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Closing,
    Watching { closed: Instant, retried: Option<Instant> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    SwitchAgain,
    Done,
}

impl CardPick {
    pub fn new(group: u32, now: Instant) -> CardPick {
        CardPick { group, start: now, state: State::Closing }
    }

    pub fn step(&mut self, now: Instant, mission_control_open: bool, group_in_front: bool) -> Step {
        if now - self.start > TIMEOUT {
            return Step::Done;
        }
        if let State::Closing = self.state {
            if mission_control_open {
                return Step::Wait;
            }
            self.state = State::Watching { closed: now, retried: None };
        }
        let State::Watching { closed, retried } = &mut self.state else { return Step::Wait };
        if now - *closed > WATCH {
            return Step::Done;
        }
        if group_in_front || retried.is_some_and(|t| now - t < RETRY) {
            return Step::Wait;
        }
        *retried = Some(now);
        Step::SwitchAgain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn nothing_to_do_when_the_group_stays_in_front() {
        let t0 = Instant::now();
        let mut p = CardPick::new(1, t0);
        assert_eq!(p.step(t0 + ms(100), true, true), Step::Wait);
        assert_eq!(p.step(t0 + ms(250), false, true), Step::Wait);
        assert_eq!(p.step(t0 + ms(601), false, true), Step::Done);
    }

    #[test]
    fn switches_again_but_not_on_every_look() {
        let t0 = Instant::now();
        let mut p = CardPick::new(1, t0);
        assert_eq!(p.step(t0 + ms(250), false, false), Step::SwitchAgain);
        assert_eq!(p.step(t0 + ms(300), false, false), Step::Wait);
        assert_eq!(p.step(t0 + ms(400), false, false), Step::SwitchAgain);
    }

    #[test]
    fn gives_up_if_mission_control_never_closes() {
        let t0 = Instant::now();
        let mut p = CardPick::new(1, t0);
        assert_eq!(p.step(t0 + ms(1700), true, false), Step::Wait);
        assert_eq!(p.step(t0 + ms(1801), true, false), Step::Done);
    }
}
