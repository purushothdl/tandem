mod app;
mod ax;
mod chrome;
mod hotkeys;
mod labels;
mod layout;
mod menu;
mod overlay;
mod writer;

use menu::Handler;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSCellImagePosition, NSImage, NSMenu, NSStatusBar, NSVariableStatusItemLength,
};
use objc2_foundation::{NSSize, NSString};

/// The icon sits in the app bundle's Resources folder, next to MacOS/.
fn load_menu_icon(mtm: MainThreadMarker) -> Option<Retained<NSImage>> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.parent()?.join("Resources/menubar.png");
    let img = NSImage::initWithContentsOfFile(mtm.alloc::<NSImage>(), &NSString::from_str(path.to_str()?))?;
    img.setTemplate(true);
    img.setSize(NSSize::new(18.0, 18.0));
    Some(img)
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    match arg.as_str() {
        "--restore" => {
            println!("{}", if chrome::restore_from_file() { "Dock and menu bar restored" } else { "Nothing to restore" });
            return;
        }
        "--check" => {
            println!("Accessibility trusted: {}", ax::is_trusted(true));
            println!("Windows on screen: {}", ax::window_order().len());
            for w in app::visible_picks() {
                println!("would tile: pid {} {:?}", w.pid, w.bounds);
            }
            return;
        }
        _ => {}
    }

    let mtm = MainThreadMarker::new().expect("main thread");
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    if !ax::is_trusted(true) {
        eprintln!("wingroup needs Accessibility permission: System Settings > Privacy & Security > Accessibility");
    }

    let bar = NSStatusBar::systemStatusBar();
    let status_item = bar.statusItemWithLength(NSVariableStatusItemLength);
    let button = status_item.button(mtm).expect("status button");
    if let Some(img) = load_menu_icon(mtm) {
        button.setImage(Some(&img));
        button.setImagePosition(NSCellImagePosition::ImageLeft);
        app::HAS_ICON.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    button.setTitle(&NSString::from_str(&app::title()));

    // built fresh each time it opens, so it always shows the current windows
    let handler = Handler::new(mtm, button.clone());
    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*handler)));
    status_item.setMenu(Some(&menu));
    hotkeys::install();

    app::start_tracking();
    nsapp.run();
    drop(handler);
}
