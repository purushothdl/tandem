//! The main display.

use super::CGRect;
use crate::core::geometry::Rect;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSScreen};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(id: u32) -> CGRect;
}

pub fn bounds() -> Rect {
    unsafe { CGDisplayBounds(CGMainDisplayID()) }.into()
}

fn main_screen() -> Option<Retained<NSScreen>> {
    NSScreen::screens(MainThreadMarker::new()?).firstObject()
}

/// The part not taken by the menu bar and the Dock.
pub fn visible() -> Rect {
    let Some(s) = main_screen() else { return bounds() };
    let (f, v) = (s.frame(), s.visibleFrame());
    Rect { x: v.origin.x, y: f.size.height - (v.origin.y + v.size.height), w: v.size.width, h: v.size.height }
}

/// Everything below the menu bar. (Under the translucent menu bar, anything we draw while
/// resizing would shine through it.)
pub fn below_menu_bar() -> Rect {
    let d = bounds();
    let top = visible().y.max(d.y);
    Rect { y: top, h: d.bottom() - top, ..d }
}

/// Width and height, in AppKit's points.
pub fn size() -> Option<(f64, f64)> {
    main_screen().map(|s| (s.frame().size.width, s.frame().size.height))
}

/// The pointer, from the top left. AppKit counts from the bottom.
pub fn pointer() -> Option<(f64, f64)> {
    let (_, h) = size()?;
    let p = NSEvent::mouseLocation();
    Some((p.x, h - p.y))
}
