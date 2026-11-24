//! While Mission Control is open, a label on each grouped window: the group's app icons
//! and name, so thumbnails that belong together can be recognised.  Purely drawn on top; Mission Control itself is untouched.

use crate::app::{self, Label};
use crate::layout::Rect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSImageView, NSPanel, NSRunningApplication, NSScreenSaverWindowLevel, NSTextField,
    NSView, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

const H: f64 = 30.0;
const ICON: f64 = 20.0;
const RING: f64 = 4.0;

struct Shown {
    pill: Retained<NSPanel>,
    width: f64,
    name: String,
    at: Rect,
    pill_on: bool,
    /// Outline for when another window of the group is being pointed at.
    ring: Retained<NSPanel>,
    ring_on: bool,
}

thread_local! {
    static SHOWN: RefCell<HashMap<u32, Shown>> = RefCell::new(HashMap::new());
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn panel(mtm: MainThreadMarker) -> Retained<NSPanel> {
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
    p.setLevel(NSScreenSaverWindowLevel);
    p.setIgnoresMouseEvents(true);
    p.setAnimationBehavior(NSWindowAnimationBehavior::None); // appear at once, no pop-in
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    p
}

fn build(mtm: MainThreadMarker, l: &Label) -> Shown {
    let text = NSTextField::labelWithString(&NSString::from_str(&l.name), mtm);
    text.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
    text.setTextColor(Some(&NSColor::whiteColor()));
    text.sizeToFit();
    let ts = text.frame().size;
    let tw = ts.width.min(220.0);
    let icons: Vec<_> = l
        .pids
        .iter()
        .filter_map(|p| NSRunningApplication::runningApplicationWithProcessIdentifier(*p)?.icon())
        .take(4)
        .collect();
    let icons_w = icons.len() as f64 * (ICON + 3.0);
    let width = 10.0 + icons_w + 4.0 + tw + 12.0;

    let view = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, width, H));
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.08, 0.88).CGColor()));
        layer.setCornerRadius(H / 2.0);
        layer.setBorderWidth(1.0);
        layer.setBorderColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.18).CGColor()));
    }
    for (i, img) in icons.iter().enumerate() {
        let iv = NSImageView::imageViewWithImage(img, mtm);
        iv.setFrame(rect(10.0 + i as f64 * (ICON + 3.0), (H - ICON) / 2.0, ICON, ICON));
        view.addSubview(&iv);
    }
    text.setFrame(rect(10.0 + icons_w + 4.0, (H - ts.height) / 2.0, tw, ts.height));
    view.addSubview(&text);
    let pill = panel(mtm);
    pill.setContentView(Some(&view));

    let ring = panel(mtm);
    // above the thumbnails, below the app icons Mission Control draws on layer 17
    ring.setLevel(16);
    let rv = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, 10.0, 10.0));
    rv.setWantsLayer(true);
    if let Some(layer) = rv.layer() {
        layer.setBorderWidth(RING);
        layer.setCornerRadius(12.0);
        layer.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
    }
    ring.setContentView(Some(&rv));
    Shown { pill, width, name: l.name.clone(), at: Rect::default(), pill_on: false, ring, ring_on: false }
}

fn set(p: &NSPanel, on: bool, was: &mut bool) {
    if on && !*was {
        p.orderFrontRegardless();
    } else if !on && *was {
        p.orderOut(None::<&AnyObject>);
    }
    *was = on;
}

/// `mx`, `my`: the pointer; `screen_h`: height of the primary display.
pub fn refresh(mtm: MainThreadMarker, mx: f64, my: f64, screen_h: f64) {
    let Some(labels) = app::mission_control_labels(mx, my) else { return };
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        if labels.is_empty() {
            for (_, mut o) in s.drain() {
                set(&o.pill, false, &mut o.pill_on);
                set(&o.ring, false, &mut o.ring_on);
            }
            return;
        }
        let hovered = labels.iter().find(|l| l.hovered).map(|l| (l.group, l.wid));
        for l in &labels {
            if s.get(&l.wid).is_none_or(|o| o.name != l.name) {
                if let Some(mut old) = s.insert(l.wid, build(mtm, l)) {
                    set(&old.pill, false, &mut old.pill_on);
                    set(&old.ring, false, &mut old.ring_on);
                }
            }
            let Some(o) = s.get_mut(&l.wid) else { continue };
            // wait until the thumbnail has stopped moving
            let settled = o.at == l.at;
            if !settled {
                o.at = l.at;
                let y = l.at.y + 10.0;
                o.pill.setFrame_display(rect(l.at.x + (l.at.w - o.width) / 2.0, screen_h - y - H, o.width, H), true);
                let r = Rect { x: l.at.x - RING, y: l.at.y - RING, w: l.at.w + 2.0 * RING, h: l.at.h + 2.0 * RING };
                o.ring.setFrame_display(rect(r.x, screen_h - r.bottom(), r.w, r.h), true);
            }
            set(&o.pill, settled, &mut o.pill_on);
            // macOS outlines the one under the pointer itself; its partners are ours
            let partner = hovered.is_some_and(|(g, wid)| g == l.group && wid != l.wid);
            set(&o.ring, settled && partner, &mut o.ring_on);
        }
    });
}
