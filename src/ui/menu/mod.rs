//! The menu-bar menu, built anew each time it opens so it always shows the current windows.

mod chooser_row;
mod dialogs;
mod group_row;
mod layout_strip;

use crate::app;
use crate::app::commands::{self, Outcome};
use crate::app::views::{self, MenuGroup, MenuWindow, app_names};
use crate::platform::{apps, corners, settings};
use crate::ui::widgets::*;
use dialogs::{Chooser, Renamer};
use group_row::{GroupRow, light};
use layout_strip::layout_strip;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBaselineOffsetAttributeName, NSCellImagePosition, NSColor, NSControlStateValueOn,
    NSEventModifierFlags, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSMenu, NSMenuDelegate,
    NSMenuItem, NSStatusBar, NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{NSMutableAttributedString, NSNumber, NSRange, NSSize, NSString, NSURL};
use std::cell::RefCell;

/// The dot's font size, and how far it is raised to the middle of the icon.
const DOT: f64 = 7.0;
const DOT_UP: f64 = 2.0;

pub struct Ivars {
    button: Retained<NSStatusBarButton>,
    /// The dialogs, while open.
    chooser: RefCell<Option<Chooser>>,
    renamer: RefCell<Option<Renamer>>,
    /// The last thing that went wrong, shown once.
    note: RefCell<Option<String>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TandemHandler"]
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
            self.report(commands::tile_visible());
        }
        #[unsafe(method(switchTo:))]
        fn switch_to(&self, i: &NSMenuItem) {
            self.report(commands::switch_to(i.tag() as u32));
        }
        #[unsafe(method(toggleFull:))]
        fn toggle_full(&self, _s: Option<&AnyObject>) {
            self.report(commands::toggle_full());
        }
        #[unsafe(method(ungroupOne:))]
        fn ungroup_one(&self, i: &NSMenuItem) {
            self.report(commands::ungroup(i.tag() as u32));
        }
        #[unsafe(method(ungroupAll:))]
        fn ungroup_all(&self, _s: Option<&AnyObject>) {
            self.report(commands::ungroup_all());
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
            self.report(commands::remove_window(i.tag() as u32));
        }
        #[unsafe(method(cancelDialog:))]
        fn cancel_dialog(&self, _s: Option<&AnyObject>) {
            self.close_dialogs();
        }
        #[unsafe(method(tileChosen:))]
        fn tile_chosen(&self, _s: Option<&AnyObject>) {
            let chosen = self.ivars().chooser.borrow().as_ref().map(|c| (dialogs::ticked(c), c.target));
            self.close_dialogs();
            match chosen {
                Some((picks, Some((id, _)))) => self.report(commands::add_windows(id, &picks)),
                Some((picks, None)) => self.report(commands::tile_chosen(&picks)),
                None => {}
            }
        }
        #[unsafe(method(renameDone:))]
        fn rename_done(&self, _s: Option<&AnyObject>) {
            let new = self.ivars().renamer.borrow().as_ref().map(|r| (r.id, r.field.stringValue().to_string()));
            self.close_dialogs();
            if let Some((id, name)) = new {
                self.report(commands::rename(id, &name));
            }
        }
        #[unsafe(method(toggleBar:))]
        fn toggle_bar(&self, _s: Option<&AnyObject>) {
            self.report(commands::toggle_bar());
        }
        #[unsafe(method(setCorners:))]
        fn set_corners(&self, i: &NSMenuItem) {
            self.report(commands::set_corners(i.tag() as u32));
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
            commands::shutdown();
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

    /// Log what was done; show what went wrong the next time the menu opens.
    pub fn report(&self, o: Outcome) {
        *self.ivars().note.borrow_mut() = o.as_ref().err().cloned();
        commands::report(o);
        self.show_status();
    }

    /// A small green dot beside the icon while there are groups.
    fn show_status(&self) {
        let button = &self.ivars().button;
        // without the icon (run outside the bundle) a glyph stands in for it
        let icon = if button.image().is_some() { "" } else { "▥" };
        let dot = if views::any_groups() { " ●" } else { "" };
        let title = NSMutableAttributedString::from_nsstring(&NSString::from_str(&format!("{icon}{dot}")));
        let range = NSRange::new(icon.encode_utf16().count(), dot.encode_utf16().count());
        // SAFETY: each value is of the type its attribute expects, the range is inside the text.
        unsafe {
            title.addAttribute_value_range(NSFontAttributeName, &NSFont::systemFontOfSize(DOT), range);
            title.addAttribute_value_range(NSForegroundColorAttributeName, &NSColor::systemGreenColor(), range);
            title.addAttribute_value_range(NSBaselineOffsetAttributeName, &NSNumber::new_f64(DOT_UP), range);
        }
        button.setAttributedTitle(&title);
    }

    fn fill(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        let m = views::menu();
        menu.removeAllItems();
        menu.setAutoenablesItems(false);
        menu.setShowsStateColumn(false);

        if !m.trusted {
            let i = self.item("Allow Tandem to move windows…", "lock.shield", Some(sel!(openPrivacy:)), 0);
            subtitle(&i, "Opens System Settings. Turn Tandem on, then reopen it");
            menu.addItem(&i);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&self.item("Quit Tandem", "power", Some(sel!(quit:)), 0));
            return;
        }
        if let Some(note) = self.ivars().note.take() {
            menu.addItem(&self.item(&note, "exclamationmark.triangle", None, 0));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }

        // the main action, with the apps it will take
        match &m.preview {
            Some(tiles) => {
                let pids = pids(tiles);
                let i = self.item(&format!("Tile {} Windows", tiles.len()), "", Some(sel!(tileVisible:)), 0);
                subtitle(&i, &short(&app_names(&pids), 34));
                i.setImage(Some(&padded(mtm, &icons(mtm, &pids, 28.0), false)));
                key(&i, "t");
                menu.addItem(&i);
            }
            None => {
                let i = self.item("Tile Windows", "rectangle.split.2x1", None, 0);
                subtitle(
                    &i,
                    if m.all_grouped {
                        "Everything in view is already grouped"
                    } else {
                        "Place two or more windows side by side first"
                    },
                );
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
        menu.addItem(&self.settings_item());
        menu.addItem(&self.item("Quit Tandem", "power", Some(sel!(quit:)), 0));
    }

    fn settings_item(&self) -> Retained<NSMenuItem> {
        let sub = NSMenu::new(self.mtm());
        sub.setAutoenablesItems(false);
        let bar = self.item("Show Groups in Mission Control", "", Some(sel!(toggleBar:)), 0);
        if settings::shows_bar() {
            bar.setState(NSControlStateValueOn);
        }
        sub.addItem(&bar);
        sub.addItem(&self.corners_item());
        let i = self.item("Settings", "gearshape", None, 0);
        i.setEnabled(true);
        i.setSubmenu(Some(&sub));
        i
    }

    fn corners_item(&self) -> Retained<NSMenuItem> {
        let sub = NSMenu::new(self.mtm());
        sub.setAutoenablesItems(false);
        let now = corners::radius();
        for (name, radius) in corners::CHOICES {
            let i = self.item(name, "", Some(sel!(setCorners:)), radius);
            if radius == now {
                i.setState(NSControlStateValueOn);
            }
            sub.addItem(&i);
        }
        sub.addItem(&NSMenuItem::separatorItem(self.mtm()));
        sub.addItem(&self.item("Reopen apps to see it", "", None, 0));
        let i = self.item("Window Corners", "", None, 0);
        i.setEnabled(true);
        i.setSubmenu(Some(&sub));
        i
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
            let mut name = apps::name(t.pid);
            // two windows of one app need telling apart
            if g.tiles.iter().filter(|o| o.pid == t.pid).count() > 1
                && let Some(title) =
                    windows.iter().find(|w| w.window.wid == t.wid).map(|w| &w.title).filter(|t| !t.is_empty())
            {
                name = format!("{name} — {}", short(title, 30));
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

/// The icon in the menu bar. It is there as long as this is kept.
pub struct StatusItem {
    _item: Retained<NSStatusItem>,
    _handler: Retained<Handler>,
}

pub fn install(mtm: MainThreadMarker) -> StatusItem {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let button = item.button(mtm).expect("a new status item has a button");
    if let Some(img) = menu_bar_icon(mtm) {
        button.setImage(Some(&img));
        button.setImagePosition(NSCellImagePosition::ImageLeft);
    }
    let handler = Handler::new(mtm, button);
    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*handler)));
    item.setMenu(Some(&menu));
    handler.show_status();
    StatusItem { _item: item, _handler: handler }
}

/// In the bundle's Resources folder, next to MacOS/.
fn menu_bar_icon(mtm: MainThreadMarker) -> Option<Retained<NSImage>> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.parent()?.join("Resources/menubar.png");
    let img = NSImage::initWithContentsOfFile(mtm.alloc::<NSImage>(), &NSString::from_str(path.to_str()?))?;
    img.setTemplate(true);
    img.setSize(NSSize::new(18.0, 18.0));
    Some(img)
}
