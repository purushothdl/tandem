//! What the user chose, kept between runs.

use objc2_foundation::{NSUserDefaults, ns_string};

/// The bar of groups in Mission Control. Off unless turned on.
pub fn shows_bar() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(ns_string!("showBarInMissionControl"))
}

pub fn set_shows_bar(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, ns_string!("showBarInMissionControl"));
}
