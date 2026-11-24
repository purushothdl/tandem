//! Pieces shared by the menu and the dialogs.

use super::rect;
use crate::core::window::WindowRef;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSControlSize, NSFont, NSImage, NSLineBreakMode, NSRunningApplication, NSScroller,
    NSScrollerPart, NSScrollerStyle, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub fn label(mtm: MainThreadMarker, text: &str, size: f64, bold: bool, quiet: bool) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    let font = if bold { NSFont::boldSystemFontOfSize(size) } else { NSFont::systemFontOfSize(size) };
    l.setFont(Some(&font));
    if quiet {
        l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    l.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    l
}

/// Cut to this many characters, with "…".
pub fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}

pub fn pids(windows: &[WindowRef]) -> Vec<i32> {
    windows.iter().map(|w| w.pid).collect()
}

pub fn icon(pid: i32, size: f64) -> Option<Retained<NSImage>> {
    let img = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?.icon()?;
    img.setSize(NSSize::new(size, size));
    Some(img)
}

/// The apps' icons in a row, every one the same size.
#[allow(deprecated)] // lockFocus is the simple way without a block
pub fn icons(mtm: MainThreadMarker, pids: &[i32], size: f64) -> Retained<NSImage> {
    const GAP: f64 = 5.0;
    let mut pids = pids.to_vec();
    pids.dedup();
    let n = pids.len().max(1) as f64;
    let img = NSImage::initWithSize(mtm.alloc::<NSImage>(), NSSize::new(n * size + (n - 1.0) * GAP, size));
    img.lockFocus();
    for (i, pid) in pids.iter().enumerate() {
        if let Some(ic) = icon(*pid, size) {
            ic.drawInRect(NSRect::new(NSPoint::new(i as f64 * (size + GAP), 0.0), NSSize::new(size, size)));
        }
    }
    img.unlockFocus();
    img
}

/// The image with room above and below. A menu row is as tall as its image, and AppKit has
/// no other way to make rows roomier.
#[allow(deprecated)]
pub fn padded(mtm: MainThreadMarker, img: &NSImage, symbol: bool) -> Retained<NSImage> {
    const PAD: f64 = 3.0;
    let s = img.size();
    let w = if symbol { s.width.max(16.0) } else { s.width };
    let out = NSImage::initWithSize(mtm.alloc::<NSImage>(), NSSize::new(w, s.height + 2.0 * PAD));
    out.lockFocus();
    img.drawInRect(rect((w - s.width) / 2.0, PAD, s.width, s.height));
    out.unlockFocus();
    out.setTemplate(symbol); // keeps following the menu's text colour
    out
}

/// Have `mouseEntered:` and `mouseExited:` called, whichever app is in front.
pub fn track_hover(v: &NSView) {
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(v),
            None,
        )
    };
    v.addTrackingArea(&area);
}

define_class!(
    // A thin, pale bar instead of the thick round one.
    #[unsafe(super(NSScroller))]
    #[thread_kind = MainThreadOnly]
    #[name = "WGSlimScroller"]
    pub struct SlimScroller;

    impl SlimScroller {
        #[unsafe(method(scrollerWidthForControlSize:scrollerStyle:))]
        fn bar_width(_size: NSControlSize, _style: NSScrollerStyle) -> f64 {
            14.0
        }
        #[unsafe(method(drawRect:))]
        fn draw(&self, _r: NSRect) {
            self.draw_bar();
        }
    }
);

impl SlimScroller {
    fn draw_bar(&self) {
        let k = self.rectForPart(NSScrollerPart::Knob);
        if k.size.height < 8.0 {
            return;
        }
        let r = rect(k.origin.x + (k.size.width - 9.0) / 2.0, k.origin.y + 2.0, 9.0, k.size.height - 4.0);
        NSColor::labelColor().colorWithAlphaComponent(0.24).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, 2.5, 2.5).fill();
    }
}
