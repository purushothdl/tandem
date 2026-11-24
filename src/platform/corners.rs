//! How round window corners are, system-wide. An undocumented AppKit setting; apps read it
//! when they start.

use objc2_foundation::{NSUserDefaults, ns_string};
use std::process::Command;

const KEY: &str = "NSConvolutionOverride1";

/// Name and radius. Zero would mean "not set", so one point is as square as it gets.
pub const CHOICES: [(&str, u32); 4] =
    [("macOS Default", 0), ("Slightly Rounded", 10), ("Almost Square", 4), ("Square", 1)];

/// Zero: macOS's own.
pub fn radius() -> u32 {
    NSUserDefaults::standardUserDefaults().integerForKey(ns_string!("NSConvolutionOverride1")).max(0) as u32
}

pub fn set_radius(radius: u32) -> bool {
    let value = radius.to_string();
    let args = if radius == 0 { vec!["delete", "-g", KEY] } else { vec!["write", "-g", KEY, "-float", &value] };
    Command::new("defaults").args(args).status().is_ok_and(|s| s.success())
}
