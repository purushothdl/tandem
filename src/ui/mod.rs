//! Everything drawn. Views come from `app::views`, actions go to `app::commands`.

pub mod menu;
pub mod mission_control;
pub mod overlay;
mod widgets;

use crate::core::geometry::Rect;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSView, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// Our rectangles count from the top of the screen, AppKit's from the bottom.
fn cocoa(r: Rect, screen_h: f64) -> NSRect {
    rect(r.x, screen_h - r.bottom(), r.w, r.h)
}

/// A see-through panel that lets every click through.
fn overlay_panel(mtm: MainThreadMarker, level: isize) -> Retained<NSPanel> {
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc(),
        rect(0.0, 0.0, 10.0, 10.0),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { p.setReleasedWhenClosed(false) };
    p.setOpaque(false);
    p.setBackgroundColor(Some(&NSColor::clearColor()));
    p.setHasShadow(false);
    p.setLevel(level);
    p.setIgnoresMouseEvents(true);
    p.setAnimationBehavior(NSWindowAnimationBehavior::None); // appear at once, no pop-in
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    p
}

fn rounded_view(mtm: MainThreadMarker, frame: NSRect, radius: f64) -> Retained<NSView> {
    let v = NSView::initWithFrame(mtm.alloc(), frame);
    v.setWantsLayer(true);
    if let Some(layer) = v.layer() {
        layer.setCornerRadius(radius);
    }
    v
}
