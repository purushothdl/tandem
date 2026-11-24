//! The split bar (on hover and while dragging) and a backdrop behind a group being resized.
//! Both are click-through; app.rs handles the mouse.

use crate::app::{self, GroupView};
use crate::layout::{Rect, Seam};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSNormalWindowLevel, NSPanel, NSScreen,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::RefCell;

const BAR_W: f64 = 3.0;

fn panel(mtm: MainThreadMarker, rect: NSRect) -> Retained<NSPanel> {
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc(),
        rect,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { p.setReleasedWhenClosed(false) };
    p.setOpaque(false);
    p.setBackgroundColor(Some(&NSColor::clearColor()));
    p.setHasShadow(false);
    p.setLevel(NSFloatingWindowLevel);
    p.setIgnoresMouseEvents(true);
    p.setAnimationBehavior(NSWindowAnimationBehavior::None);
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    p
}

struct GroupOverlay {
    bars: Vec<Retained<NSPanel>>,
    /// Plain panel behind the group while it resizes, so a slow app never shows what is underneath.
    backdrop: Retained<NSPanel>,
    bar_shown: Option<(usize, Rect)>,
    backdrop_on: bool,
}

thread_local! {
    static OV: RefCell<Vec<GroupOverlay>> = const { RefCell::new(Vec::new()) };
}

fn primary_height(mtm: MainThreadMarker) -> f64 {
    let screens = NSScreen::screens(mtm);
    if screens.count() == 0 { 0.0 } else { screens.objectAtIndex(0).frame().size.height }
}

/// Accessibility coordinates have their origin top-left; AppKit's is bottom-left.
fn cocoa(r: Rect, screen_h: f64) -> NSRect {
    NSRect::new(NSPoint::new(r.x, screen_h - (r.y + r.h)), NSSize::new(r.w, r.h))
}

fn build(mtm: MainThreadMarker, v: &GroupView) -> GroupOverlay {
    let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(BAR_W, 10.0));
    let bars = v
        .seams
        .iter()
        .map(|_| {
            let p = panel(mtm, zero);
            // the user's own accent colour, so it matches the rest of macOS
            p.setBackgroundColor(Some(&NSColor::controlAccentColor()));
            p
        })
        .collect();
    let backdrop = panel(mtm, zero);
    backdrop.setLevel(NSNormalWindowLevel);
    backdrop.setOpaque(true);
    backdrop.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
    GroupOverlay { bars, backdrop, bar_shown: None, backdrop_on: false }
}

fn bar_rect(s: &Seam) -> Rect {
    let (across, len) = (s.at - BAR_W / 2.0, s.to - s.from);
    if s.side {
        Rect { x: across, y: s.from, w: BAR_W, h: len }
    } else {
        Rect { x: s.from, y: across, w: len, h: BAR_W }
    }
}

fn hide(o: &GroupOverlay) {
    o.bars.iter().for_each(|b| b.orderOut(None::<&AnyObject>));
    o.backdrop.orderOut(None::<&AnyObject>);
}

/// `mx`, `my`: the pointer, in accessibility coordinates.
pub fn refresh_at(mx: f64, my: f64) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(views) = app::snapshot(mx, my) else { return };
    OV.with(|ov| {
        let mut ov = ov.borrow_mut();
        let same = ov.len() == views.len() && ov.iter().zip(&views).all(|(o, v)| o.bars.len() == v.seams.len());
        if !same {
            ov.iter().for_each(hide);
            *ov = views.iter().map(|v| build(mtm, v)).collect();
        }
        let h = primary_height(mtm);
        for (o, v) in ov.iter_mut().zip(&views) {
            match v.backdrop_below {
                Some(wid) if !o.backdrop_on => {
                    o.backdrop.setFrame_display(cocoa(v.region, h), true);
                    o.backdrop.orderWindow_relativeTo(NSWindowOrderingMode::Below, wid as isize);
                    o.backdrop_on = true;
                }
                None if o.backdrop_on => {
                    o.backdrop.orderOut(None::<&AnyObject>);
                    o.backdrop_on = false;
                }
                _ => {}
            }
            let want = v.hot.and_then(|k| v.seams.get(k).map(|s| (k, bar_rect(s))));
            if want == o.bar_shown {
                continue;
            }
            if let Some((k, _)) = o.bar_shown {
                if want.map(|w| w.0) != Some(k) {
                    o.bars[k].orderOut(None::<&AnyObject>);
                }
            }
            if let Some((k, r)) = want {
                o.bars[k].setFrame_display(cocoa(r, h), true);
                if o.bar_shown.map(|b| b.0) != Some(k) {
                    o.bars[k].orderFrontRegardless();
                }
            }
            o.bar_shown = want;
        }
    });
}
