//! A group in the menu: click to switch, hover for its submenu.

use super::Handler;
use crate::app::commands;
use crate::app::views::MenuGroup;
use crate::ui::rect;
use crate::ui::widgets::*;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent, NSImage, NSImageView, NSMenu, NSMenuItem, NSTextField,
    NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSString};
use std::cell::Cell;

/// Longer group names end in "…", so one long name cannot widen the whole menu.
const MAX_NAME: f64 = 170.0;
const MORE_W: f64 = 34.0;

pub struct GroupRowIvars {
    id: u32,
    active: bool,
    owner: Retained<Handler>,
    hover: Cell<bool>,
    name: Retained<NSTextField>,
    state: Retained<NSTextField>,
    more: Retained<NSImageView>,
}

define_class!(
    // A group in the menu: click to switch.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TandemGroupRow"]
    #[ivars = GroupRowIvars]
    pub struct GroupRow;

    impl GroupRow {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _r: NSRect) {
            let b = self.bounds();
            if self.ivars().hover.get() {
                NSColor::controlAccentColor().setFill();
                let r = rect(5.0, 0.0, b.size.width - 10.0, b.size.height);
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, 7.0, 7.0).fill();
            }
        }
        #[unsafe(method(mouseEntered:))]
        fn entered(&self, _e: &NSEvent) {
            if let Some(i) = self.enclosingMenuItem()
                && let Some(menu) = unsafe { i.menu() } {
                    light(&menu, Some(&i));
                }
        }
        #[unsafe(method(mouseExited:))]
        fn exited(&self, _e: &NSEvent) {
            // macOS keeps the row lit while its submenu is in use
            if !self.enclosingMenuItem().is_some_and(|i| i.isHighlighted()) {
                self.set_hover(false);
            }
        }
        #[unsafe(method(mouseUp:))]
        fn up(&self, _e: &NSEvent) {
            if let Some(menu) = self.enclosingMenuItem().and_then(|i| unsafe { i.menu() }) {
                menu.cancelTracking();
            }
            self.ivars().owner.report(commands::switch_to(self.ivars().id));
        }
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, p: NSPoint) -> *const NSView {
            let hit: *const NSView = unsafe { msg_send![super(self), hitTest: p] };
            if hit.is_null() {
                return hit;
            }
            let me: &NSView = self;
            me
        }
    }
);

impl GroupRow {
    pub fn new(mtm: MainThreadMarker, g: &MenuGroup, owner: &Handler) -> Retained<Self> {
        const LEFT: f64 = 16.0;
        const ICON: f64 = 22.0;
        let h = if g.active { 46.0 } else { 36.0 };
        let img = icons(mtm, &pids(&g.tiles), ICON);
        let iw = img.size().width;
        let name = label(mtm, &g.name, 13.0, false, false);
        name.sizeToFit();
        let nw = name.frame().size.width.min(MAX_NAME);
        let w = (LEFT + iw + 10.0 + nw + 16.0 + MORE_W + 6.0).max(260.0);
        let x = LEFT + iw + 10.0;
        let tw = w - x - MORE_W - 14.0;
        let state = label(mtm, "● In use", 11.0, true, false);
        if g.active {
            name.setFrame(rect(x, h / 2.0, tw, 17.0));
            state.setFrame(rect(x, h / 2.0 - 17.0, tw, 15.0));
        } else {
            name.setFrame(rect(x, (h - 17.0) / 2.0, tw, 17.0));
            state.setHidden(true);
        }
        for l in [&name, &state] {
            l.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        }
        let more = NSImageView::initWithFrame(mtm.alloc(), rect(w - MORE_W - 5.0, (h - 20.0) / 2.0, 28.0, 20.0));
        more.setImage(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("chevron.right"), None)
                .as_deref(),
        );
        more.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);

        let this = Self::alloc(mtm).set_ivars(GroupRowIvars {
            id: g.id,
            active: g.active,
            owner: owner.retain(),
            hover: Cell::new(false),
            name: name.clone(),
            state: state.clone(),
            more: more.clone(),
        });
        let v: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, w, h)] };
        if name.fittingSize().width > tw {
            v.setToolTip(Some(&NSString::from_str(&g.name)));
        }
        v.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        iv.setFrame(rect(LEFT, (h - ICON) / 2.0, iw, ICON));
        v.addSubview(&iv);
        v.addSubview(&name);
        v.addSubview(&state);
        v.addSubview(&more);
        track_hover(&v);
        v.show();
        v
    }

    fn set_hover(&self, on: bool) {
        if self.ivars().hover.replace(on) != on {
            self.show();
        }
    }

    fn show(&self) {
        let iv = self.ivars();
        let (text, blue, quiet) = if iv.hover.get() {
            (NSColor::whiteColor(), NSColor::whiteColor(), NSColor::whiteColor())
        } else {
            (NSColor::labelColor(), NSColor::controlAccentColor(), NSColor::secondaryLabelColor())
        };
        iv.name.setTextColor(Some(&text));
        if iv.active {
            iv.state.setTextColor(Some(&blue));
        }
        iv.more.setContentTintColor(Some(&quiet));
        self.setNeedsDisplay(true);
    }
}

/// Highlight this row and no other.
pub fn light(menu: &NSMenu, item: Option<&NSMenuItem>) {
    for i in menu.itemArray().iter() {
        if let Some(row) = i.view().and_then(|v| v.downcast::<GroupRow>().ok()) {
            row.set_hover(item.is_some_and(|it| std::ptr::eq::<NSMenuItem>(it, &*i)));
        }
    }
}
