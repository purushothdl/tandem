//! The menu and the "Choose Windows" panel.

use crate::app::{self, MenuGroup, MenuWindow, Tile};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, Message, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAppearanceCustomization, NSAutoresizingMaskOptions, NSBackingStoreType, NSBezierPath, NSButton, NSColor, NSControlSize,
    NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSFont, NSImage,
    NSImageSymbolConfiguration, NSImageView, NSLineBreakMode, NSMenu, NSMenuDelegate, NSMenuItem, NSPanel, NSRunningApplication, NSScrollView, NSScroller, NSScrollerPart, NSScrollerStyle,
    NSStatusBarButton, NSTextField, NSTextFieldBezelStyle, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use std::cell::{Cell, RefCell};
use std::sync::atomic::Ordering;

struct Renamer {
    panel: Retained<NSPanel>,
    field: Retained<NSTextField>,
    id: u32,
}

struct Chooser {
    panel: Retained<NSPanel>,
    rows: Vec<Retained<Row>>,
    /// Add to this group (with this much room left) instead of making a new one.
    target: Option<(u32, usize)>,
    tile: Retained<NSButton>,
}

pub struct Ivars {
    button: Retained<NSStatusBarButton>,
    /// The "Choose Windows" window: each row's checkbox with its (pid, window id).
    chooser: RefCell<Option<Chooser>>,
    renamer: RefCell<Option<Renamer>>,
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
        // also for the arrow keys, and while the pointer is in a row's submenu
        #[unsafe(method(menu:willHighlightItem:))]
        fn will_highlight(&self, menu: &NSMenu, item: Option<&NSMenuItem>) {
            light(menu, item);
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
        fn toggle_full(&self, _s: Option<&AnyObject>) {
            self.say(app::toggle_full());
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
            self.show_renamer(i.tag() as u32);
        }
        #[unsafe(method(openChooser:))]
        fn open_chooser(&self, _s: Option<&AnyObject>) {
            self.show_chooser(None);
        }
        #[unsafe(method(addWindows:))]
        fn add_windows(&self, i: &NSMenuItem) {
            self.show_chooser(Some(i.tag() as u32));
        }
        #[unsafe(method(removeWindow:))]
        fn remove_window(&self, i: &NSMenuItem) {
            self.say(app::remove_window(i.tag() as u32));
        }
        #[unsafe(method(cancelDialog:))]
        fn cancel_dialog(&self, _s: Option<&AnyObject>) {
            self.close_dialogs();
        }
        #[unsafe(method(tileChosen:))]
        fn tile_chosen(&self, _s: Option<&AnyObject>) {
            let chosen = self.ivars().chooser.borrow().as_ref().map(|c| (ticked_rows(c), c.target));
            self.close_dialogs();
            match chosen {
                Some((picks, Some((id, _)))) => self.say(app::add_windows(id, &picks)),
                Some((picks, None)) => self.say(app::tile_chosen(&picks)),
                None => {}
            }
        }
        #[unsafe(method(renameDone:))]
        fn rename_done(&self, _s: Option<&AnyObject>) {
            let new = self.ivars().renamer.borrow().as_ref().map(|r| (r.id, r.field.stringValue().to_string()));
            self.close_dialogs();
            if let Some((id, name)) = new {
                self.say(app::rename(id, &name));
            }
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
            renamer: RefCell::default(),
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
        menu.setShowsStateColumn(false);

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
                i.setImage(Some(&padded(mtm, &icons(mtm, tiles, 28.0), false)));
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
            // the one in use first
            let (used, rest): (Vec<&MenuGroup>, Vec<&MenuGroup>) = m.groups.iter().partition(|g| g.active);
            for g in used.into_iter().chain(rest) {
                menu.addItem(&self.group_item(mtm, g, &m.windows));
            }
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        if !m.groups.is_empty() {
            let (t, sym) = if m.full {
                ("Leave Full Screen", "arrow.down.right.and.arrow.up.left")
            } else {
                ("Full Screen", "arrow.up.left.and.arrow.down.right")
            };
            let i = self.item(t, sym, Some(sel!(toggleFull:)), 0);
            subtitle(&i, if m.full { "Shows the Dock again" } else { "All groups, with the Dock hidden" });
            menu.addItem(&i);
        }
        if m.groups.len() > 1 {
            menu.addItem(&self.item("Ungroup All", "rectangle.dashed", Some(sel!(ungroupAll:)), 0));
        }
        menu.addItem(&self.item("Quit wingroup", "power", Some(sel!(quit:)), 0));
    }

    fn close_dialogs(&self) {
        if let Some(c) = self.ivars().chooser.take() {
            c.panel.close();
        }
        if let Some(r) = self.ivars().renamer.take() {
            r.panel.close();
        }
        app::MODAL.store(false, Ordering::Relaxed);
    }

    /// The shared look of our dialogs: solid, big title, quiet line under it, buttons below.
    /// Returns the panel, its content view and the default button.
    fn dialog(&self, w: f64, h: f64, title: &str, hint: &str, ok: &str, action: Sel) -> Dialog {
        self.close_dialogs();
        let mtm = self.mtm();
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame(0.0, 0.0, w, h),
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
        let bg = if dark { NSColor::colorWithSRGBRed_green_blue_alpha(0.165, 0.169, 0.188, 1.0) } else { NSColor::whiteColor() };
        panel.setBackgroundColor(Some(&bg));
        let view = NSView::initWithFrame(mtm.alloc(), frame(0.0, 0.0, w, h));

        let t = label(mtm, title, 20.0, true, false);
        t.setFrame(frame(MARGIN, h - 32.0 - 28.0, w - 2.0 * MARGIN, 28.0));
        view.addSubview(&t);
        let l = label(mtm, hint, 13.0, false, true);
        l.setFrame(frame(MARGIN, h - 32.0 - 28.0 - 6.0 - 18.0, w - 2.0 * MARGIN, 18.0));
        view.addSubview(&l);

        let button = |t: &str, action: Sel, key: &str, x: f64, bw: f64| {
            let b = unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(t), Some(self), Some(action), mtm) };
            b.setControlSize(NSControlSize::Large);
            b.setFrame(frame(x, 22.0, bw, 34.0));
            b.setKeyEquivalent(&NSString::from_str(key));
            view.addSubview(&b);
            b
        };
        button("Cancel", sel!(cancelDialog:), "\u{1b}", w - MARGIN - 140.0 - 10.0 - 90.0, 90.0);
        let ok = button(ok, action, "\r", w - MARGIN - 140.0, 140.0);
        Dialog { panel, view, ok }
    }

    fn open(&self, d: &Dialog) {
        d.panel.setContentView(Some(&d.view));
        d.panel.center();
        app::MODAL.store(true, Ordering::Relaxed);
        d.panel.makeKeyAndOrderFront(None);
    }

    /// Stays open: pick rows, then Tile.
    fn show_chooser(&self, add_to: Option<u32>) {
        let mtm = self.mtm();
        let m = app::menu_model();
        let group = add_to.and_then(|id| m.groups.iter().find(|g| g.id == id));
        let target = group.map(|g| (g.id, app::MAX_TILES.saturating_sub(g.tiles.len())));
        let (title, hint) = match group {
            Some(g) => ("Add Windows", format!("They join “{}” next to the one you used last.", g.name)),
            None => ("Choose Windows", "Select two or more windows to tile together.".to_string()),
        };
        let windows: Vec<MenuWindow> =
            m.windows.into_iter().filter(|w| !group.is_some_and(|g| g.tiles.iter().any(|t| t.wid == w.wid))).collect();
        const ROW: f64 = 56.0;
        const W: f64 = 420.0;
        let content_h = windows.len().max(1) as f64 * ROW;
        let list_h = content_h.min(6.0 * ROW);
        let h = HEAD + list_h + FOOT;
        let d = self.dialog(W, h, title, &hint, "Tile", sel!(tileChosen:));

        let list = NSView::initWithFrame(mtm.alloc(), frame(0.0, 0.0, W, content_h));
        let mut rows = Vec::new();
        for (i, w) in windows.iter().enumerate() {
            let y = content_h - (i as f64 + 1.0) * ROW + 2.0;
            let r = Row::new(mtm, frame(MARGIN - 12.0, y, W - 2.0 * (MARGIN - 12.0), ROW - 4.0), w, self);
            list.addSubview(&r);
            rows.push(r);
        }
        let scroll = NSScrollView::initWithFrame(mtm.alloc(), frame(0.0, FOOT, W, list_h));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(content_h > list_h);
        scroll.setAutohidesScrollers(true);
        let bar: Retained<SlimScroller> = unsafe { msg_send![mtm.alloc::<SlimScroller>(), initWithFrame: frame(0.0, 0.0, 10.0, list_h)] };
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

    fn show_renamer(&self, id: u32) {
        let Some(now) = app::name_of(id) else { return };
        let mtm = self.mtm();
        const W: f64 = 380.0;
        let d = self.dialog(W, HEAD + 120.0 + FOOT, "Rename Group", "The name appears in Mission Control and the menu.", "Rename", sel!(renameDone:));
        let img = icons(mtm, &app::tiles_of(id), 44.0);
        let size = img.size();
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        iv.setFrame(frame(MARGIN, FOOT + 62.0, size.width, size.height));
        d.view.addSubview(&iv);
        let field = NSTextField::textFieldWithString(&NSString::from_str(&now), mtm);
        field.setFont(Some(&NSFont::systemFontOfSize(15.0)));
        field.setControlSize(NSControlSize::Large);
        field.setBezelStyle(NSTextFieldBezelStyle::RoundedBezel);
        field.setFrame(frame(MARGIN, FOOT + 12.0, W - 2.0 * MARGIN, 34.0));
        d.view.addSubview(&field);
        self.open(&d);
        d.panel.makeFirstResponder(Some(&field));
        unsafe { field.selectText(None) };
        *self.ivars().renamer.borrow_mut() = Some(Renamer { panel: d.panel, field, id });
    }

    fn update_chooser(&self) {
        if let Some(c) = &*self.ivars().chooser.borrow() {
            let n = ticked_rows(c).len();
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

    /// A row that switches to the group when clicked; its submenu opens beside it on hover.
    fn group_item(&self, mtm: MainThreadMarker, g: &MenuGroup, windows: &[MenuWindow]) -> Retained<NSMenuItem> {
        let i = self.item(&g.name, "", Some(sel!(switchTo:)), g.id);
        i.setSubmenu(Some(&self.group_menu(mtm, g, windows)));
        i.setEnabled(true);
        i.setView(Some(&GroupRow::new(mtm, g, self)));
        i
    }

    /// A group's other commands.
    fn group_menu(&self, mtm: MainThreadMarker, g: &MenuGroup, windows: &[MenuWindow]) -> Retained<NSMenu> {
        let sub = NSMenu::new(mtm);
        sub.setAutoenablesItems(false);
        sub.setShowsStateColumn(false);
        if !g.layouts.is_empty() {
            sub.addItem(&NSMenuItem::sectionHeaderWithTitle(&NSString::from_str("Layout   ⌃⌥L"), mtm));
            let i = self.item("Layout", "", None, 0);
            i.setEnabled(true);
            i.setView(Some(&layout_strip(mtm, g, self)));
            sub.addItem(&i);
            sub.addItem(&NSMenuItem::separatorItem(mtm));
        }
        let room = g.tiles.len() < app::MAX_TILES;
        sub.addItem(&self.item("Add Windows…", "plus.rectangle.on.rectangle", room.then_some(sel!(addWindows:)), g.id));
        let out = NSMenu::new(mtm);
        out.setAutoenablesItems(false);
        out.setShowsStateColumn(false);
        for t in &g.tiles {
            let mut name = app::app_name(t.pid);
            // two windows of one app need telling apart
            if g.tiles.iter().filter(|o| o.pid == t.pid).count() > 1 {
                if let Some(title) = windows.iter().find(|w| w.wid == t.wid).map(|w| &w.title).filter(|t| !t.is_empty()) {
                    name = format!("{name} — {}", title.chars().take(36).collect::<String>());
                }
            }
            let i = self.item(&name, "", Some(sel!(removeWindow:)), t.wid);
            i.setImage(icon(t.pid, 18.0).map(|g| padded(mtm, &g, false)).as_deref());
            out.addItem(&i);
        }
        let r = self.item("Remove from Group", "minus.rectangle", None, 0);
        r.setEnabled(true);
        r.setSubmenu(Some(&out));
        sub.addItem(&r);
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&self.item("Even Out Sizes", "equal.square", Some(sel!(equalize:)), g.id));
        sub.addItem(&self.item("Rename…", "pencil", Some(sel!(rename:)), g.id));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&self.item("Ungroup", "rectangle.dashed", Some(sel!(ungroupOne:)), g.id));
        sub
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
            i.setImage(img.map(|g| padded(mtm, &g, true)).as_deref());
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

define_class!(
    // A thin, pale bar instead of the thick round one.
    #[unsafe(super(NSScroller))]
    #[thread_kind = MainThreadOnly]
    #[name = "WGSlimScroller"]
    struct SlimScroller;

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
        let r = frame(k.origin.x + (k.size.width - 9.0) / 2.0, k.origin.y + 2.0, 9.0, k.size.height - 4.0);
        NSColor::labelColor().colorWithAlphaComponent(0.24).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, 2.5, 2.5).fill();
    }
}

/// The layouts as a row of small pictures.
fn layout_strip(mtm: MainThreadMarker, g: &MenuGroup, owner: &Handler) -> Retained<NSView> {
    const W: f64 = 46.0;
    const H: f64 = 32.0;
    const GAP: f64 = 6.0;
    let n = g.layouts.len() as f64;
    let strip = NSView::initWithFrame(mtm.alloc(), frame(0.0, 0.0, 28.0 + n * W + (n - 1.0) * GAP, H + 10.0));
    for (k, l) in g.layouts.iter().enumerate() {
        let t = LayoutThumb::new(mtm, frame(14.0 + k as f64 * (W + GAP), 5.0, W, H), g.id, k, l, owner);
        strip.addSubview(&t);
    }
    strip
}

pub struct ThumbIvars {
    group: u32,
    index: usize,
    rects: Vec<crate::layout::Rect>,
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
    #[name = "WGLayoutThumb"]
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
                let t = frame(4.0 + r.x * w + 1.0, 4.0 + (1.0 - r.y - r.h) * h + 1.0, r.w * w - 2.0, r.h * h - 2.0);
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
            iv.owner.say(app::set_layout(iv.group, iv.index));
        }
    }
);

impl LayoutThumb {
    fn new(mtm: MainThreadMarker, r: NSRect, group: u32, index: usize, l: &app::LayoutChoice, owner: &Handler) -> Retained<Self> {
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
        v
    }
}

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
    #[name = "WGGroupRow"]
    #[ivars = GroupRowIvars]
    struct GroupRow;

    impl GroupRow {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _r: NSRect) {
            let b = self.bounds();
            if self.ivars().hover.get() {
                NSColor::controlAccentColor().setFill();
                let r = frame(5.0, 0.0, b.size.width - 10.0, b.size.height);
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, 7.0, 7.0).fill();
            }
        }
        #[unsafe(method(mouseEntered:))]
        fn entered(&self, _e: &NSEvent) {
            if let Some(i) = self.enclosingMenuItem() {
                if let Some(menu) = unsafe { i.menu() } {
                    light(&menu, Some(&i));
                }
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
            self.ivars().owner.say(app::switch_to(self.ivars().id));
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
    fn new(mtm: MainThreadMarker, g: &MenuGroup, owner: &Handler) -> Retained<Self> {
        const LEFT: f64 = 16.0;
        const ICON: f64 = 22.0;
        let h = if g.active { 46.0 } else { 36.0 };
        let img = icons(mtm, &g.tiles, ICON);
        let iw = img.size().width;
        let name = label(mtm, &g.name, 13.0, false, false);
        name.sizeToFit();
        let nw = name.frame().size.width.min(240.0);
        let w = (LEFT + iw + 10.0 + nw + 16.0 + MORE_W + 6.0).max(260.0);
        let x = LEFT + iw + 10.0;
        let tw = w - x - MORE_W - 14.0;
        let state = label(mtm, "● In use", 11.0, true, false);
        if g.active {
            name.setFrame(frame(x, h / 2.0, tw, 17.0));
            state.setFrame(frame(x, h / 2.0 - 17.0, tw, 15.0));
        } else {
            name.setFrame(frame(x, (h - 17.0) / 2.0, tw, 17.0));
            state.setHidden(true);
        }
        for l in [&name, &state] {
            l.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        }
        let more = NSImageView::initWithFrame(mtm.alloc(), frame(w - MORE_W - 5.0, (h - 20.0) / 2.0, 28.0, 20.0));
        more.setImage(NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("chevron.right"), None).as_deref());
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
        let v: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame(0.0, 0.0, w, h)] };
        v.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        iv.setFrame(frame(LEFT, (h - ICON) / 2.0, iw, ICON));
        v.addSubview(&iv);
        v.addSubview(&name);
        v.addSubview(&state);
        v.addSubview(&more);
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&v),
                None,
            )
        };
        v.addTrackingArea(&area);
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

const MARGIN: f64 = 28.0;
/// Space above a dialog's content (title and hint) and below it (buttons).
const HEAD: f64 = 32.0 + 28.0 + 6.0 + 18.0 + 20.0;
const FOOT: f64 = 78.0;

struct Dialog {
    panel: Retained<NSPanel>,
    view: Retained<NSView>,
    ok: Retained<NSButton>,
}

pub struct RowIvars {
    pid: i32,
    wid: u32,
    on: Cell<bool>,
    hover: Cell<bool>,
    check: Retained<NSImageView>,
    name: Retained<NSTextField>,
    title: Retained<NSTextField>,
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
    fn new(mtm: MainThreadMarker, r: NSRect, w: &MenuWindow, owner: &Handler) -> Retained<Self> {
        const ICON: f64 = 38.0;
        let (width, h) = (r.size.width, r.size.height);
        let check = NSImageView::initWithFrame(mtm.alloc(), frame(width - 38.0, (h - 24.0) / 2.0, 24.0, 24.0));
        let (x, tw) = (12.0 + ICON + 14.0, width - (12.0 + ICON + 14.0) - 50.0);
        let name = label(mtm, &w.app, 15.0, true, false);
        let title = label(mtm, &w.title, 12.0, false, true);
        if w.title.is_empty() {
            name.setFrame(frame(x, (h - 20.0) / 2.0, tw, 20.0));
        } else {
            name.setFrame(frame(x, h / 2.0 + 0.0, tw, 20.0));
            title.setFrame(frame(x, h / 2.0 - 18.0, tw, 16.0));
        }
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            pid: w.pid,
            wid: w.wid,
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
        if let Some(img) = icon(w.pid, ICON) {
            let iv = NSImageView::imageViewWithImage(&img, mtm);
            iv.setFrame(frame(12.0, (h - ICON) / 2.0, ICON, ICON));
            v.addSubview(&iv);
        }
        v.addSubview(&name);
        v.addSubview(&title);
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

/// Highlight this row and no other.
fn light(menu: &NSMenu, item: Option<&NSMenuItem>) {
    for i in menu.itemArray().iter() {
        if let Some(row) = i.view().and_then(|v| v.downcast::<GroupRow>().ok()) {
            row.set_hover(item.is_some_and(|it| std::ptr::eq::<NSMenuItem>(it, &*i)));
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

/// The image with room above and below. A menu row is as tall as its image, and AppKit has
/// no other way to make rows roomier.
#[allow(deprecated)]
fn padded(mtm: MainThreadMarker, img: &NSImage, symbol: bool) -> Retained<NSImage> {
    const PAD: f64 = 3.0;
    let s = img.size();
    let w = if symbol { s.width.max(16.0) } else { s.width };
    let out = NSImage::initWithSize(mtm.alloc::<NSImage>(), NSSize::new(w, s.height + 2.0 * PAD));
    out.lockFocus();
    img.drawInRect(frame((w - s.width) / 2.0, PAD, s.width, s.height));
    out.unlockFocus();
    out.setTemplate(symbol); // keeps following the menu's text colour
    out
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
