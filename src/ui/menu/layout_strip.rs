//! The layouts of a group as small pictures.

use super::Handler;
use crate::app::commands;
use crate::app::views::{LayoutChoice, MenuGroup};
use crate::core::geometry::Rect;
use crate::ui::rect;
use crate::ui::widgets::*;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSEvent, NSView};
use objc2_foundation::{NSRect, NSString};
use std::cell::Cell;

/// The layouts as small pictures, four to a row.
pub fn layout_strip(mtm: MainThreadMarker, g: &MenuGroup, owner: &Handler) -> Retained<NSView> {
    const W: f64 = 46.0;
    const H: f64 = 32.0;
    const GAP: f64 = 6.0;
    const PER_ROW: usize = 4;
    let rows = g.layouts.len().div_ceil(PER_ROW);
    let cols = g.layouts.len().min(PER_ROW) as f64;
    let h = rows as f64 * (H + GAP) - GAP + 10.0;
    let strip = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, 28.0 + cols * W + (cols - 1.0) * GAP, h));
    for (k, l) in g.layouts.iter().enumerate() {
        let x = 14.0 + (k % PER_ROW) as f64 * (W + GAP);
        let y = h - 5.0 - H - (k / PER_ROW) as f64 * (H + GAP);
        let t = LayoutThumb::new(mtm, rect(x, y, W, H), g.id, k, l, owner);
        strip.addSubview(&t);
    }
    strip
}

pub struct ThumbIvars {
    group: u32,
    index: usize,
    rects: Vec<Rect>,
    main: bool,
    fits: bool,
    current: bool,
    hover: Cell<bool>,
    owner: Retained<Handler>,
}

define_class!(
    // One layout, drawn small. Click to use it.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TandemLayoutThumb"]
    #[ivars = ThumbIvars]
    struct LayoutThumb;

    impl LayoutThumb {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _r: NSRect) {
            let iv = self.ivars();
            let b = self.bounds();
            let hot = iv.hover.get() && iv.fits;
            let base = if hot || iv.current { NSColor::controlAccentColor() } else { NSColor::labelColor() };
            let dim = if iv.fits { 1.0 } else { 0.35 };
            let plate = if hot { base.colorWithAlphaComponent(0.16) } else { NSColor::labelColor().colorWithAlphaComponent(0.06 * dim) };
            plate.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(b, 6.0, 6.0).fill();
            let (w, h) = (b.size.width - 8.0, b.size.height - 8.0);
            for (i, r) in iv.rects.iter().enumerate() {
                // AppKit draws from the bottom
                let t = rect(4.0 + r.x * w + 1.0, 4.0 + (1.0 - r.y - r.h) * h + 1.0, r.w * w - 2.0, r.h * h - 2.0);
                let strong = if iv.main && i == 0 { 0.85 } else { 0.45 };
                base.colorWithAlphaComponent(strong * dim).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(t, 2.0, 2.0).fill();
            }
        }
        #[unsafe(method(mouseEntered:))]
        fn entered(&self, _e: &NSEvent) {
            self.ivars().hover.set(true);
            self.setNeedsDisplay(true);
        }
        #[unsafe(method(mouseExited:))]
        fn exited(&self, _e: &NSEvent) {
            self.ivars().hover.set(false);
            self.setNeedsDisplay(true);
        }
        #[unsafe(method(mouseUp:))]
        fn up(&self, _e: &NSEvent) {
            let iv = self.ivars();
            if !iv.fits {
                return;
            }
            // close the whole menu, not just this submenu
            let item = unsafe { self.superview() }.and_then(|s| s.enclosingMenuItem());
            if let Some(menu) = item.and_then(|i| unsafe { i.menu() }) {
                if let Some(top) = unsafe { menu.supermenu() } {
                    top.cancelTracking();
                }
                menu.cancelTracking();
            }
            iv.owner.report(commands::set_layout(iv.group, iv.index));
        }
    }
);

impl LayoutThumb {
    fn new(
        mtm: MainThreadMarker,
        r: NSRect,
        group: u32,
        index: usize,
        l: &LayoutChoice,
        owner: &Handler,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ThumbIvars {
            group,
            index,
            rects: l.rects.clone(),
            main: l.main,
            fits: l.fits,
            current: l.current,
            hover: Cell::new(false),
            owner: owner.retain(),
        });
        let v: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: r] };
        if !l.fits {
            v.setToolTip(Some(&NSString::from_str("These apps cannot get that small")));
        }
        track_hover(&v);
        v
    }
}
