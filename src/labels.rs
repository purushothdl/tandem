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
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const H: f64 = 30.0;
const ICON: f64 = 20.0;
const RING: f64 = 4.0;

struct Shown {
    pill: Retained<NSPanel>,
    width: f64,
    name: String,
    at: Rect,
    pill_on: f64,
    /// Outline for when another window of the group is being pointed at.
    ring: Retained<NSPanel>,
    ring_on: f64,
}

thread_local! {
    static SHOWN: RefCell<HashMap<u32, Shown>> = RefCell::new(HashMap::new());
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

pub fn panel(mtm: MainThreadMarker) -> Retained<NSPanel> {
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
    Shown { pill, width, name: l.name.clone(), at: Rect::default(), pill_on: 0.0, ring, ring_on: 0.0 }
}

/// Move a panel's opacity a step towards where it should be.
fn fade(p: &NSPanel, on: bool, now: &mut f64) {
    let next = if on { (*now + 0.16).min(1.0) } else { (*now - 0.22).max(0.0) };
    if next == *now {
        return;
    }
    if *now == 0.0 {
        p.setAlphaValue(0.0);
        p.orderFrontRegardless();
    }
    p.setAlphaValue(next);
    if next == 0.0 {
        p.orderOut(None::<&AnyObject>);
    }
    *now = next;
}

fn hide(o: &mut Shown) {
    for (p, now) in [(&o.pill, &mut o.pill_on), (&o.ring, &mut o.ring_on)] {
        if *now > 0.0 {
            p.orderOut(None::<&AnyObject>);
            *now = 0.0;
        }
    }
}

/// How long every thumbnail must have been still before anything shows. A slow swipe keeps
/// them moving, with short pauses; showing in each pause is what flickered.
const STILL: Duration = Duration::from_millis(220);

thread_local! {
    static MOVED: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// `mx`, `my`: the pointer; `screen_h`: height of the primary display.
pub fn refresh(mtm: MainThreadMarker, mx: f64, my: f64, screen_h: f64) {
    let Some(labels) = app::mission_control_labels(mx, my) else { return };
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        if labels.is_empty() {
            // kept for next time: building them again is slow enough to see
            s.values_mut().for_each(hide);
            crate::cards::hide();
            MOVED.set(None);
            return;
        }
        s.retain(|wid, o| {
            let keep = labels.iter().any(|l| l.wid == *wid && l.name == o.name);
            if !keep {
                hide(o);
            }
            keep
        });
        for l in &labels {
            let o = s.entry(l.wid).or_insert_with(|| build(mtm, l));
            if o.at != l.at {
                o.at = l.at;
                MOVED.set(Some(Instant::now()));
            }
        }
        if MOVED.get().is_none() {
            MOVED.set(Some(Instant::now()));
        }
        let still = MOVED.get().is_none_or(|t| t.elapsed() > STILL);
        // pointing at a group in the bar outlines all of its windows
        let calm = MOVED.get().map_or(Duration::ZERO, |t| t.elapsed());
        let bar = crate::cards::refresh(mtm, calm, mx, my, screen_h);
        if bar.away {
            s.values_mut().for_each(hide);
            return;
        }
        let hovered = labels.iter().find(|l| l.hovered).map(|l| (l.group, l.wid));
        for l in &labels {
            let Some(o) = s.get_mut(&l.wid) else { continue };
            if still && o.pill_on == 0.0 && o.ring_on == 0.0 {
                let y = l.at.y + 10.0;
                o.pill.setFrame_display(rect(l.at.x + (l.at.w - o.width) / 2.0, screen_h - y - H, o.width, H), true);
                let r = Rect { x: l.at.x - RING, y: l.at.y - RING, w: l.at.w + 2.0 * RING, h: l.at.h + 2.0 * RING };
                o.ring.setFrame_display(rect(r.x, screen_h - r.bottom(), r.w, r.h), true);
            }
            // the bar says it already
            fade(&o.pill, still && !bar.showing, &mut o.pill_on);
            // macOS outlines the one under the pointer itself; its partners are ours
            let partner = bar.hot == Some(l.group) || hovered.is_some_and(|(g, wid)| g == l.group && wid != l.wid);
            fade(&o.ring, still && partner, &mut o.ring_on);
        }
    });
}
