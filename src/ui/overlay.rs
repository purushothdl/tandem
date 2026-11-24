//! Over the groups: the split bar (on hover and while dragging), a backdrop behind a group
//! being resized, and where a dragged window would go. All of it lets clicks through.

use super::{cocoa, overlay_panel, rect, rounded_view};
use crate::app::views::{self, Overlay};
use crate::core::geometry::{Axis, Rect};
use crate::core::layout::Seam;
use crate::platform::screen;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSColor, NSFloatingWindowLevel, NSNormalWindowLevel, NSPanel, NSWindowOrderingMode};
use std::cell::RefCell;

const BAR_W: f64 = 3.0;

struct Panels {
    bars: Vec<Retained<NSPanel>>,
    /// Plain panel behind the group while it resizes, so a slow app never shows what is underneath.
    backdrop: Retained<NSPanel>,
    /// Marks the tile a dragged one would trade places with.
    target: Retained<NSPanel>,
    target_shown: Option<Rect>,
    bar_shown: Option<(usize, Rect)>,
    backdrop_on: bool,
}

thread_local! {
    static OV: RefCell<Vec<Panels>> = const { RefCell::new(Vec::new()) };
}

fn build(mtm: MainThreadMarker, v: &Overlay) -> Panels {
    let bars = v
        .seams
        .iter()
        .map(|_| {
            let p = overlay_panel(mtm, NSFloatingWindowLevel);
            // the user's own accent colour, so it matches the rest of macOS
            p.setBackgroundColor(Some(&NSColor::controlAccentColor()));
            p
        })
        .collect();
    let backdrop = overlay_panel(mtm, NSFloatingWindowLevel);
    backdrop.setLevel(NSNormalWindowLevel);
    backdrop.setOpaque(true);
    backdrop.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
    let target = overlay_panel(mtm, NSFloatingWindowLevel);
    let tv = rounded_view(mtm, rect(0.0, 0.0, 10.0, 10.0), 12.0);
    if let Some(l) = tv.layer() {
        let accent = NSColor::controlAccentColor();
        l.setBackgroundColor(Some(&accent.colorWithAlphaComponent(0.22).CGColor()));
        l.setBorderColor(Some(&accent.colorWithAlphaComponent(0.9).CGColor()));
        l.setBorderWidth(3.0);
    }
    target.setContentView(Some(&tv));
    Panels { target, target_shown: None, bars, backdrop, bar_shown: None, backdrop_on: false }
}

fn bar_rect(s: &Seam) -> Rect {
    let (across, len) = (s.at - BAR_W / 2.0, s.to - s.from);
    if s.axis == Axis::X {
        Rect { x: across, y: s.from, w: BAR_W, h: len }
    } else {
        Rect { x: s.from, y: across, w: len, h: BAR_W }
    }
}

fn hide(o: &Panels) {
    o.bars.iter().for_each(|b| b.orderOut(None::<&AnyObject>));
    o.backdrop.orderOut(None::<&AnyObject>);
    o.target.orderOut(None::<&AnyObject>);
}

/// With the pointer here.
pub fn refresh(x: f64, y: f64) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(views) = views::overlays(x, y) else { return };
    let Some((_, h)) = screen::size() else { return };
    OV.with(|ov| {
        let mut ov = ov.borrow_mut();
        let same = ov.len() == views.len() && ov.iter().zip(&views).all(|(o, v)| o.bars.len() == v.seams.len());
        if !same {
            ov.iter().for_each(hide);
            *ov = views.iter().map(|v| build(mtm, v)).collect();
        }
        for (o, v) in ov.iter_mut().zip(&views) {
            show_backdrop(o, v, h);
            show_target(o, v, h);
            show_bar(o, v, h);
        }
    });
}

fn show_backdrop(o: &mut Panels, v: &Overlay, h: f64) {
    match v.backdrop_below {
        Some(wid) if !o.backdrop_on => {
            o.backdrop.setFrame_display(cocoa(v.region, h), true);
            o.backdrop.orderWindow_relativeTo(NSWindowOrderingMode::Below, wid as isize);
        }
        None if o.backdrop_on => o.backdrop.orderOut(None::<&AnyObject>),
        _ => {}
    }
    o.backdrop_on = v.backdrop_below.is_some();
}

fn show_target(o: &mut Panels, v: &Overlay, h: f64) {
    if v.drop == o.target_shown {
        return;
    }
    match v.drop {
        Some(r) => {
            let inset = Rect { x: r.x + 6.0, y: r.y + 6.0, w: r.w - 12.0, h: r.h - 12.0 };
            o.target.setFrame_display(cocoa(inset, h), true);
            o.target.orderFrontRegardless();
        }
        None => o.target.orderOut(None::<&AnyObject>),
    }
    o.target_shown = v.drop;
}

fn show_bar(o: &mut Panels, v: &Overlay, h: f64) {
    let want = v.hot.and_then(|k| v.seams.get(k).map(|s| (k, bar_rect(s))));
    if want == o.bar_shown {
        return;
    }
    let (old, new) = (o.bar_shown.map(|b| b.0), want.map(|w| w.0));
    if let Some(k) = old.filter(|_| old != new) {
        o.bars[k].orderOut(None::<&AnyObject>);
    }
    if let Some((k, r)) = want {
        o.bars[k].setFrame_display(cocoa(r, h), true);
        if old != new {
            o.bars[k].orderFrontRegardless();
        }
    }
    o.bar_shown = want;
}
