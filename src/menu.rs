//! The menu and the "Choose Windows" panel.

use crate::app::{self, MenuGroup, MenuWindow, Tile};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, Message, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType, NSButton, NSColor, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSFont, NSImage,
    NSImageView, NSLineBreakMode, NSMenu, NSMenuDelegate, NSMenuItem, NSPanel, NSRunningApplication, NSScrollView,
    NSStatusBarButton, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use std::cell::{Cell, RefCell};

struct Chooser {
    panel: Retained<NSPanel>,
    rows: Vec<Retained<Row>>,
    tile: Retained<NSButton>,
}

pub struct Ivars {
    button: Retained<NSStatusBarButton>,
    /// The "Choose Windows" window: each row's checkbox with its (pid, window id).
    chooser: RefCell<Option<Chooser>>,
    /// The last thing that went wrong, shown once.
    note: RefCell<Option<String>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WGHandler"]
    #[ivars = Ivars]
    pub struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    unsafe impl NSMenuDelegate for Handler {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.fill(menu);
        }
    }

    impl Handler {
        #[unsafe(method(tileVisible:))]
        fn tile_visible(&self, _s: Option<&AnyObject>) {
            self.say(app::tile_visible());
        }
        #[unsafe(method(switchTo:))]
        fn switch_to(&self, i: &NSMenuItem) {
            self.say(app::switch_to(i.tag() as u32));
        }
        #[unsafe(method(equalize:))]
        fn equalize(&self, i: &NSMenuItem) {
            self.say(app::equalize(Some(i.tag() as u32)));
        }
        #[unsafe(method(toggleFull:))]
        fn toggle_full(&self, i: &NSMenuItem) {
            self.say(app::toggle_full(Some(i.tag() as u32)));
        }
        #[unsafe(method(ungroupOne:))]
        fn ungroup_one(&self, i: &NSMenuItem) {
            self.say(app::ungroup(Some(i.tag() as u32)));
        }
        #[unsafe(method(ungroupAll:))]
        fn ungroup_all(&self, _s: Option<&AnyObject>) {
            self.say(app::ungroup_all());
        }
        #[unsafe(method(rename:))]
        fn rename(&self, i: &NSMenuItem) {
            let id = i.tag() as u32;
            if let Some(name) = app::name_of(id).and_then(|now| ask_name(self.mtm(), &now)) {
                self.say(app::rename(id, &name));
            }
        }
        #[unsafe(method(openChooser:))]
        fn open_chooser(&self, _s: Option<&AnyObject>) {
            self.show_chooser();
        }
        #[unsafe(method(cancelChooser:))]
        fn cancel_chooser(&self, _s: Option<&AnyObject>) {
            if let Some(c) = self.ivars().chooser.take() {
                c.panel.close();
            }
        }
        #[unsafe(method(tileChosen:))]
        fn tile_chosen(&self, _s: Option<&AnyObject>) {
            let Some(c) = self.ivars().chooser.take() else { return };
            let picks: Vec<(i32, u32)> = ticked_rows(&c);
            c.panel.close();
            self.say(app::tile_chosen(&picks));
        }
        #[unsafe(method(openPrivacy:))]
        fn open_privacy(&self, _s: Option<&AnyObject>) {
            let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
            if let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) {
                NSWorkspace::sharedWorkspace().openURL(&u);
            }
        }
        #[unsafe(method(quit:))]
        fn quit(&self, _s: Option<&AnyObject>) {
            app::shutdown();
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Handler {
    pub fn new(mtm: MainThreadMarker, button: Retained<NSStatusBarButton>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            button,
            chooser: RefCell::default(),
            note: RefCell::default(),
        });
        unsafe { msg_send![super(this), init] }
    }

    pub fn say(&self, msg: String) {
        app::log(&msg);
        let bad = msg.starts_with("Need") || msg.starts_with("Place") || msg.starts_with("No ");
        *self.ivars().note.borrow_mut() = bad.then_some(msg);
        self.ivars().button.setTitle(&NSString::from_str(&app::title()));
    }

    fn fill(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        let m = app::menu_model();
        menu.removeAllItems();
        menu.setAutoenablesItems(false);

        if !m.trusted {
            let i = self.item("Allow wingroup to move windows…", "lock.shield", Some(sel!(openPrivacy:)), 0);
            subtitle(&i, "Opens System Settings. Turn wingroup on, then reopen it");
            menu.addItem(&i);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&self.item("Quit wingroup", "power", Some(sel!(quit:)), 0));
            return;
        }
        if let Some(note) = self.ivars().note.take() {
            menu.addItem(&self.item(&note, "exclamationmark.triangle", None, 0));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }

        // the main action, with the apps it will take
        match &m.preview {
            Some(tiles) => {
                let i = self.item(&format!("Tile {} Windows", tiles.len()), "", Some(sel!(tileVisible:)), 0);
                subtitle(&i, &names(tiles));
                i.setImage(Some(&icons(mtm, tiles, 28.0)));
                key(&i, "t");
                menu.addItem(&i);
            }
            None => {
                let i = self.item("Tile Windows", "rectangle.split.2x1", None, 0);
                subtitle(&i, if m.all_grouped { "Everything in view is already grouped" } else { "Place two or more windows side by side first" });
                menu.addItem(&i);
            }
        }

        menu.addItem(&self.item("Choose Windows…", "checklist", Some(sel!(openChooser:)), 0));

        if !m.groups.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&NSMenuItem::sectionHeaderWithTitle(&NSString::from_str("Groups"), mtm));
            for g in &m.groups {
                menu.addItem(&self.group_item(mtm, g));
            }
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        if m.groups.len() > 1 {
            menu.addItem(&self.item("Ungroup All", "rectangle.dashed", Some(sel!(ungroupAll:)), 0));
        }
        menu.addItem(&self.item("Quit wingroup", "power", Some(sel!(quit:)), 0));
    }

    /// A window that stays open: pick rows, then Tile.
    fn show_chooser(&self) {
        let mtm = self.mtm();
        if let Some(c) = self.ivars().chooser.take() {
            c.panel.close();
        }
        let windows = app::menu_model().windows;
        const ROW: f64 = 52.0;
        const W: f64 = 400.0;
        const HEAD: f64 = 78.0;
        const FOOT: f64 = 64.0;
        let content_h = windows.len().max(1) as f64 * ROW;
        let list_h = content_h.min(7.0 * ROW);
        let h = HEAD + list_h + FOOT;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame(0.0, 0.0, W, h),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setTitle(&NSString::from_str("Choose Windows"));
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        panel.setTitlebarAppearsTransparent(true);
        panel.setMovableByWindowBackground(true);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setHidesOnDeactivate(false);

        // the same frosted background as system panels
        let view = NSVisualEffectView::initWithFrame(mtm.alloc(), frame(0.0, 0.0, W, h));
        view.setMaterial(NSVisualEffectMaterial::Popover);
        view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        view.setState(NSVisualEffectState::Active);

        let title = label(mtm, "Choose Windows", 17.0, true, false);
        title.setFrame(frame(24.0, h - 52.0, W - 48.0, 22.0));
        view.addSubview(&title);
        let hint = label(mtm, "Select two or more to tile together", 12.0, false, true);
        hint.setFrame(frame(24.0, h - 72.0, W - 48.0, 16.0));
        view.addSubview(&hint);

        let list = NSView::initWithFrame(mtm.alloc(), frame(0.0, 0.0, W, content_h));
        let mut rows = Vec::new();
        for (i, w) in windows.iter().enumerate() {
            let r = Row::new(mtm, frame(12.0, content_h - (i as f64 + 1.0) * ROW + 2.0, W - 24.0, ROW - 4.0), w, self);
            list.addSubview(&r);
            rows.push(r);
        }
        let scroll = NSScrollView::initWithFrame(mtm.alloc(), frame(0.0, FOOT, W, list_h));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(content_h > list_h);
        scroll.setAutohidesScrollers(true);
        scroll.setDocumentView(Some(&list));
        // start at the top of the list
        list.scrollPoint(NSPoint::new(0.0, content_h));
        view.addSubview(&scroll);

        let button = |t: &str, action: Sel, key: &str, x: f64, w: f64| {
            let b = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(t), Some(self), Some(action), mtm) };
            b.setControlSize(NSControlSize::Large);
            b.setFrame(frame(x, 16.0, w, 32.0));
            b.setKeyEquivalent(&NSString::from_str(key));
            view.addSubview(&b);
            b
        };
        button("Cancel", sel!(cancelChooser:), "\u{1b}", W - 250.0, 90.0);
        let tile = button("Tile", sel!(tileChosen:), "\r", W - 154.0, 134.0);

        panel.setContentView(Some(&view));
        panel.center();
        NSApplication::sharedApplication(mtm).activate();
        panel.makeKeyAndOrderFront(None);
        *self.ivars().chooser.borrow_mut() = Some(Chooser { panel, rows, tile });
        self.update_chooser();
    }

    fn update_chooser(&self) {
        if let Some(c) = &*self.ivars().chooser.borrow() {
            let n = ticked_rows(c).len();
            c.tile.setEnabled((2..=app::MAX_TILES).contains(&n));
            let t = if n < 2 { "Tile".to_string() } else { format!("Tile {n} Windows") };
            c.tile.setTitle(&NSString::from_str(&t));
        }
    }

    /// Clicking opens the group's own commands.
    fn group_item(&self, mtm: MainThreadMarker, g: &MenuGroup) -> Retained<NSMenuItem> {
        let i = self.item(&g.name, "", None, g.id);
        i.setEnabled(true);
        i.setImage(Some(&icons(mtm, &g.tiles, 22.0)));
        i.setState(if g.active { NSControlStateValueOn } else { NSControlStateValueOff });
        let sub = NSMenu::new(mtm);
        sub.setAutoenablesItems(false);
        sub.addItem(&self.item("Switch To", "arrow.up.forward.app", Some(sel!(switchTo:)), g.id));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&self.item("Even Out Sizes", "equal.square", Some(sel!(equalize:)), g.id));
        let full = if g.full { "Leave Full Screen" } else { "Full Screen" };
        sub.addItem(&self.item(full, "arrow.up.left.and.arrow.down.right", Some(sel!(toggleFull:)), g.id));
        sub.addItem(&self.item("Rename…", "pencil", Some(sel!(rename:)), g.id));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&self.item("Ungroup", "rectangle.dashed", Some(sel!(ungroupOne:)), g.id));
        i.setSubmenu(Some(&sub));
        i
    }

    fn item(&self, title: &str, symbol: &str, action: Option<Sel>, tag: u32) -> Retained<NSMenuItem> {
        let mtm = self.mtm();
        let i = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::from_str(""),
            )
        };
        unsafe { i.setTarget(Some(self)) };
        i.setTag(tag as isize);
        i.setEnabled(action.is_some());
        if !symbol.is_empty() {
            let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None);
            i.setImage(img.as_deref());
        }
        i
    }
}

fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn label(mtm: MainThreadMarker, text: &str, size: f64, bold: bool, quiet: bool) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    let font = if bold { NSFont::boldSystemFontOfSize(size) } else { NSFont::systemFontOfSize(size) };
    l.setFont(Some(&font));
    if quiet {
        l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    l.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    l
}

pub struct RowIvars {
    pid: i32,
    wid: u32,
    on: Cell<bool>,
    hover: Cell<bool>,
    check: Retained<NSImageView>,
    owner: Retained<Handler>,
}

define_class!(
    // One window in the chooser. The whole row is the button.
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
    fn new(mtm: MainThreadMarker, r: NSRect, w: &MenuWindow, owner: &Handler) -> Retained<Self> {
        let check = NSImageView::initWithFrame(mtm.alloc(), frame(r.size.width - 34.0, (r.size.height - 20.0) / 2.0, 20.0, 20.0));
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            pid: w.pid,
            wid: w.wid,
            on: Cell::new(false),
            hover: Cell::new(false),
            check: check.clone(),
            owner: owner.retain(),
        });
        let v: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: r] };
        v.setWantsLayer(true);
        if let Some(l) = v.layer() {
            l.setCornerRadius(10.0);
        }
        let h = r.size.height;
        if let Some(img) = icon(w.pid, 32.0) {
            let iv = NSImageView::imageViewWithImage(&img, mtm);
            iv.setFrame(frame(12.0, (h - 32.0) / 2.0, 32.0, 32.0));
            v.addSubview(&iv);
        }
        let tw = r.size.width - 56.0 - 44.0;
        let name = label(mtm, &w.app, 13.0, true, false);
        if w.title.is_empty() {
            name.setFrame(frame(56.0, (h - 17.0) / 2.0, tw, 17.0));
        } else {
            name.setFrame(frame(56.0, h / 2.0, tw, 17.0));
            let t = label(mtm, &w.title, 11.0, false, true);
            t.setFrame(frame(56.0, h / 2.0 - 16.0, tw, 15.0));
            v.addSubview(&t);
        }
        v.addSubview(&name);
        v.addSubview(&check);
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::MouseEnteredAndExited | NSTrackingAreaOptions::ActiveAlways | NSTrackingAreaOptions::InVisibleRect,
                Some(&v),
                None,
            )
        };
        v.addTrackingArea(&area);
        v.show();
        v
    }

    fn show(&self) {
        let iv = self.ivars();
        let on = iv.on.get();
        let name = if on { "checkmark.circle.fill" } else { "circle" };
        iv.check.setImage(NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None).as_deref());
        let tint = if on { NSColor::controlAccentColor() } else { NSColor::tertiaryLabelColor() };
        iv.check.setContentTintColor(Some(&tint));
        let bg = if on {
            NSColor::controlAccentColor().colorWithAlphaComponent(0.16)
        } else if iv.hover.get() {
            NSColor::labelColor().colorWithAlphaComponent(0.06)
        } else {
            NSColor::clearColor()
        };
        if let Some(l) = self.layer() {
            l.setBackgroundColor(Some(&bg.CGColor()));
        }
    }
}

fn ticked_rows(c: &Chooser) -> Vec<(i32, u32)> {
    c.rows.iter().map(|r| r.ivars()).filter(|r| r.on.get()).map(|r| (r.pid, r.wid)).collect()
}

fn subtitle(i: &NSMenuItem, text: &str) {
    // macOS 14.4 and later
    let has: bool = unsafe { msg_send![i, respondsToSelector: sel!(setSubtitle:)] };
    if has {
        i.setSubtitle(Some(&NSString::from_str(text)));
    }
}

fn key(i: &NSMenuItem, k: &str) {
    i.setKeyEquivalent(&NSString::from_str(k));
    i.setKeyEquivalentModifierMask(NSEventModifierFlags::Control | NSEventModifierFlags::Option);
}

fn names(tiles: &[Tile]) -> String {
    let mut out: Vec<String> = Vec::new();
    for t in tiles {
        let n = app::app_name(t.pid);
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out.join(" + ")
}

fn icon(pid: i32, size: f64) -> Option<Retained<NSImage>> {
    let img = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?.icon()?;
    img.setSize(NSSize::new(size, size));
    Some(img)
}

/// The apps' icons in a row, every one the same size.
#[allow(deprecated)] // lockFocus is the simple way without a block
fn icons(mtm: MainThreadMarker, tiles: &[Tile], size: f64) -> Retained<NSImage> {
    const GAP: f64 = 5.0;
    let mut pids: Vec<i32> = tiles.iter().map(|t| t.pid).collect();
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

fn ask_name(mtm: MainThreadMarker, now: &str) -> Option<String> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Rename group"));
    alert.setInformativeText(&NSString::from_str("The name shows on its windows in Mission Control."));
    alert.addButtonWithTitle(&NSString::from_str("Rename"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let field = NSTextField::textFieldWithString(&NSString::from_str(now), mtm);
    field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(240.0, 24.0)));
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));
    NSApplication::sharedApplication(mtm).activate();
    (alert.runModal() == NSAlertFirstButtonReturn).then(|| field.stringValue().to_string())
}
