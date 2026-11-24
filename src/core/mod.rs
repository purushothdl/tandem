//! The decisions. Nothing in here calls macOS, sleeps or reads the clock: everything comes
//! in as an argument, so all of it runs in a plain unit test.

pub mod bar_phase;
pub mod bar_spot;
pub mod card_pick;
pub mod geometry;
pub mod gesture;
pub mod glide;
pub mod layout;
pub mod preview;
pub mod recheck;
pub mod refusal;
pub mod window;
