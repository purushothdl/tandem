//! One window in the chooser. The whole row is the button.

use super::Handler;
use crate::app::views::MenuWindow;
use crate::core::window::WindowRef;
use crate::ui::rect;
use crate::ui::widgets::*;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSColor, NSEvent, NSImage, NSImageSymbolConfiguration, NSImageView, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSString};
use std::cell::Cell;

pub struct RowIvars {
    window: WindowRef,
    on: Cell<bool>,
    hover: Cell<bool>,
    check: Retained<NSImageView>,
    name: Retained<NSTextField>,
    title: Retained<NSTextField>,
    owner: Retained<Handler>,
}

define_class!(
    // A window to tick.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WGRow"]
    #[ivars = RowIvars]
    pub struct Row;

    impl Row {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _e: &NSEvent) {
            self.ivars().on.set(!self.ivars().on.get());
            self.show();
            self.ivars().owner.update_chooser();
        }
        // labels and icons inside must not take the click from the row
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, p: NSPoint) -> *const NSView {
            let hit: *const NSView = unsafe { msg_send![super(self), hitTest: p] };
            if hit.is_null() {
                return hit;
            }
            let me: &NSView = self;
            me
        }
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _e: &NSEvent) {
            self.ivars().hover.set(true);
            self.show();
        }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _e: &NSEvent) {
            self.ivars().hover.set(false);
            self.show();
        }
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move(&self) -> bool {
            false
        }
        #[unsafe(method(acceptsFirstMouse:))]
        fn first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }
    }
);

impl Row {
    /// The window, if it is ticked.
    pub fn ticked(&self) -> Option<WindowRef> {
        self.ivars().on.get().then_some(self.ivars().window)
    }

    pub fn new(mtm: MainThreadMarker, r: NSRect, w: &MenuWindow, owner: &Handler) -> Retained<Self> {
        const ICON: f64 = 38.0;
        let (width, h) = (r.size.width, r.size.height);
        let check = NSImageView::initWithFrame(mtm.alloc(), rect(width - 38.0, (h - 24.0) / 2.0, 24.0, 24.0));
        let (x, tw) = (12.0 + ICON + 14.0, width - (12.0 + ICON + 14.0) - 50.0);
        let name = label(mtm, &w.app, 15.0, true, false);
        let title = label(mtm, &w.title, 12.0, false, true);
        if w.title.is_empty() {
            name.setFrame(rect(x, (h - 20.0) / 2.0, tw, 20.0));
        } else {
            name.setFrame(rect(x, h / 2.0 + 0.0, tw, 20.0));
            title.setFrame(rect(x, h / 2.0 - 18.0, tw, 16.0));
        }
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            window: w.window,
            on: Cell::new(false),
            hover: Cell::new(false),
            check: check.clone(),
            name: name.clone(),
            title: title.clone(),
            owner: owner.retain(),
        });
        let v: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: r] };
        v.setWantsLayer(true);
        if let Some(l) = v.layer() {
            l.setCornerRadius(12.0);
        }
        if let Some(img) = icon(w.window.pid, ICON) {
            let iv = NSImageView::imageViewWithImage(&img, mtm);
            iv.setFrame(rect(12.0, (h - ICON) / 2.0, ICON, ICON));
            v.addSubview(&iv);
        }
        v.addSubview(&name);
        v.addSubview(&title);
        v.addSubview(&check);
        track_hover(&v);
        v.show();
        v
    }

    fn show(&self) {
        let iv = self.ivars();
        let on = iv.on.get();
        let symbol = if on { "checkmark.square.fill" } else { "square" };
        let cfg = NSImageSymbolConfiguration::configurationWithPointSize_weight(19.0, 0.0);
        let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None);
        iv.check.setImage(img.as_deref());
        iv.check.setSymbolConfiguration(Some(&cfg));
        // selected: solid accent with white on it, like a selected row in Finder
        let (text, quiet, tint) = if on {
            (NSColor::whiteColor(), NSColor::colorWithWhite_alpha(1.0, 0.8), NSColor::whiteColor())
        } else {
            (NSColor::labelColor(), NSColor::secondaryLabelColor(), NSColor::tertiaryLabelColor())
        };
        iv.name.setTextColor(Some(&text));
        iv.title.setTextColor(Some(&quiet));
        iv.check.setContentTintColor(Some(&tint));
        let bg = if on {
            NSColor::selectedContentBackgroundColor()
        } else if iv.hover.get() {
            NSColor::labelColor().colorWithAlphaComponent(0.07)
        } else {
            NSColor::clearColor()
        };
        if let Some(l) = self.layer() {
            l.setBackgroundColor(Some(&bg.CGColor()));
        }
    }
}
