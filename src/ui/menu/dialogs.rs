//! Choose Windows, Add Windows and Rename.

use super::Handler;
use super::chooser_row::Row;
use crate::app::views::{self, MenuWindow};
use crate::app::{self};
use crate::core::window::WindowRef;
use crate::ui::rect;
use crate::ui::widgets::*;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{DefinedClass, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBackingStoreType, NSButton, NSColor, NSControlSize, NSFloatingWindowLevel, NSFont,
    NSImageView, NSPanel, NSScrollView, NSScrollerStyle, NSTextField, NSTextFieldBezelStyle, NSView, NSWindowButton,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSString};
use std::sync::atomic::Ordering;

const MARGIN: f64 = 28.0;
/// Space above a dialog's content (title and hint) and below it (buttons).
const HEAD: f64 = 32.0 + 28.0 + 6.0 + 18.0 + 20.0;
const FOOT: f64 = 78.0;

pub(super) struct Dialog {
    pub(super) panel: Retained<NSPanel>,
    pub(super) view: Retained<NSView>,
    pub(super) ok: Retained<NSButton>,
}

pub(super) struct Chooser {
    pub(super) panel: Retained<NSPanel>,
    pub(super) rows: Vec<Retained<Row>>,
    /// Add to this group (with this much room left) instead of making a new one.
    pub(super) target: Option<(u32, usize)>,
    pub(super) tile: Retained<NSButton>,
}

pub(super) struct Renamer {
    pub(super) panel: Retained<NSPanel>,
    pub(super) field: Retained<NSTextField>,
    pub(super) id: u32,
}

impl Handler {
    pub(super) fn close_dialogs(&self) {
        if let Some(c) = self.ivars().chooser.take() {
            c.panel.close();
        }
        if let Some(r) = self.ivars().renamer.take() {
            r.panel.close();
        }
        app::DIALOG_OPEN.store(false, Ordering::Relaxed);
    }

    /// The shared look of our dialogs: solid, big title, quiet line under it, buttons below.
    pub(super) fn dialog(&self, w: f64, h: f64, title: &str, hint: &str, ok: &str, action: Sel) -> Dialog {
        self.close_dialogs();
        let mtm = self.mtm();
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect(0.0, 0.0, w, h),
            // non-activating: it takes the keyboard and the first click at once, whatever app is in use
            NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setTitle(&NSString::from_str(title));
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        panel.setTitlebarAppearsTransparent(true);
        panel.setMovableByWindowBackground(true);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setHidesOnDeactivate(false);
        for b in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(b) = panel.standardWindowButton(b) {
                b.setHidden(true);
            }
        }

        // solid, so it looks the same over anything
        let dark = panel.effectiveAppearance().name().to_string().contains("Dark");
        let bg = if dark {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.165, 0.169, 0.188, 1.0)
        } else {
            NSColor::whiteColor()
        };
        panel.setBackgroundColor(Some(&bg));
        let view = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, w, h));

        let t = label(mtm, title, 20.0, true, false);
        t.setFrame(rect(MARGIN, h - 32.0 - 28.0, w - 2.0 * MARGIN, 28.0));
        view.addSubview(&t);
        let l = label(mtm, hint, 13.0, false, true);
        l.setFrame(rect(MARGIN, h - 32.0 - 28.0 - 6.0 - 18.0, w - 2.0 * MARGIN, 18.0));
        view.addSubview(&l);

        let button = |t: &str, action: Sel, key: &str, x: f64, bw: f64| {
            let b = unsafe {
                NSButton::buttonWithTitle_target_action(&NSString::from_str(t), Some(self), Some(action), mtm)
            };
            b.setControlSize(NSControlSize::Large);
            b.setFrame(rect(x, 22.0, bw, 34.0));
            b.setKeyEquivalent(&NSString::from_str(key));
            view.addSubview(&b);
            b
        };
        button("Cancel", sel!(cancelDialog:), "\u{1b}", w - MARGIN - 140.0 - 10.0 - 90.0, 90.0);
        let ok = button(ok, action, "\r", w - MARGIN - 140.0, 140.0);
        Dialog { panel, view, ok }
    }

    pub(super) fn open(&self, d: &Dialog) {
        d.panel.setContentView(Some(&d.view));
        d.panel.center();
        app::DIALOG_OPEN.store(true, Ordering::Relaxed);
        d.panel.makeKeyAndOrderFront(None);
    }

    /// Stays open: pick rows, then Tile.
    pub(super) fn show_chooser(&self, add_to: Option<u32>) {
        let mtm = self.mtm();
        let m = views::menu();
        let group = add_to.and_then(|id| m.groups.iter().find(|g| g.id == id));
        let target = group.map(|g| (g.id, app::MAX_TILES.saturating_sub(g.tiles.len())));
        let (title, hint) = match group {
            Some(g) => ("Add Windows", format!("They join “{}” next to the one you used last.", g.name)),
            None => ("Choose Windows", "Select two or more windows to tile together.".to_string()),
        };
        let windows: Vec<MenuWindow> = m
            .windows
            .into_iter()
            .filter(|w| !group.is_some_and(|g| g.tiles.iter().any(|t| t.wid == w.window.wid)))
            .collect();
        const ROW: f64 = 56.0;
        const W: f64 = 420.0;
        let content_h = windows.len().max(1) as f64 * ROW;
        let list_h = content_h.min(6.0 * ROW);
        let h = HEAD + list_h + FOOT;
        let d = self.dialog(W, h, title, &hint, "Tile", sel!(tileChosen:));

        let list = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, W, content_h));
        let mut rows = Vec::new();
        for (i, w) in windows.iter().enumerate() {
            let y = content_h - (i as f64 + 1.0) * ROW + 2.0;
            let r = Row::new(mtm, rect(MARGIN - 12.0, y, W - 2.0 * (MARGIN - 12.0), ROW - 4.0), w, self);
            list.addSubview(&r);
            rows.push(r);
        }
        let scroll = NSScrollView::initWithFrame(mtm.alloc(), rect(0.0, FOOT, W, list_h));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(content_h > list_h);
        scroll.setAutohidesScrollers(true);
        let bar: Retained<SlimScroller> =
            unsafe { msg_send![mtm.alloc::<SlimScroller>(), initWithFrame: rect(0.0, 0.0, 10.0, list_h)] };
        bar.setScrollerStyle(NSScrollerStyle::Legacy);
        scroll.setVerticalScroller(Some(&bar));
        scroll.setScrollerStyle(NSScrollerStyle::Legacy);
        scroll.setDocumentView(Some(&list));
        // start at the top of the list
        list.scrollPoint(NSPoint::new(0.0, content_h));
        d.view.addSubview(&scroll);

        self.open(&d);
        *self.ivars().chooser.borrow_mut() = Some(Chooser { panel: d.panel, rows, target, tile: d.ok });
        self.update_chooser();
    }

    pub(super) fn show_renamer(&self, id: u32) {
        let Some(now) = views::name_of(id) else { return };
        let mtm = self.mtm();
        const W: f64 = 380.0;
        let d = self.dialog(
            W,
            HEAD + 120.0 + FOOT,
            "Rename Group",
            "The name appears in Mission Control and the menu.",
            "Rename",
            sel!(renameDone:),
        );
        let img = icons(mtm, &views::pids_of(id), 44.0);
        let size = img.size();
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        iv.setFrame(rect(MARGIN, FOOT + 62.0, size.width, size.height));
        d.view.addSubview(&iv);
        let field = NSTextField::textFieldWithString(&NSString::from_str(&now), mtm);
        field.setFont(Some(&NSFont::systemFontOfSize(15.0)));
        field.setControlSize(NSControlSize::Large);
        field.setBezelStyle(NSTextFieldBezelStyle::RoundedBezel);
        field.setFrame(rect(MARGIN, FOOT + 12.0, W - 2.0 * MARGIN, 34.0));
        d.view.addSubview(&field);
        self.open(&d);
        d.panel.makeFirstResponder(Some(&field));
        unsafe { field.selectText(None) };
        *self.ivars().renamer.borrow_mut() = Some(Renamer { panel: d.panel, field, id });
    }

    pub(super) fn update_chooser(&self) {
        if let Some(c) = &*self.ivars().chooser.borrow() {
            let n = ticked(c).len();
            let (verb, range) = match c.target {
                Some((_, room)) => ("Add", 1..=room),
                None => ("Tile", 2..=app::MAX_TILES),
            };
            c.tile.setEnabled(range.contains(&n));
            let t = match n {
                0 => verb.to_string(),
                1 => format!("{verb} 1 Window"),
                _ => format!("{verb} {n} Windows"),
            };
            c.tile.setTitle(&NSString::from_str(&t));
        }
    }
}

pub(super) fn ticked(c: &Chooser) -> Vec<WindowRef> {
    c.rows.iter().filter_map(|r| r.ticked()).collect()
}
