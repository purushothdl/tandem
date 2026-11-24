//! Tandem: windows that stay together. See README.md and docs/ARCHITECTURE.md.

mod app;
mod core;
mod platform;
mod ui;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use platform::element::is_trusted;
use platform::{dock, windows};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--restore") => {
            println!("{}", if dock::restore_after_crash() { "Dock restored" } else { "Nothing to restore" });
        }
        Some("--check") => check(),
        _ => run(),
    }
}

fn check() {
    println!("Accessibility trusted: {}", is_trusted(true));
    println!("Windows on screen: {}", windows::ordinary().len());
    for w in app::commands::visible_picks() {
        println!("would tile: pid {} {:?}", w.pid, w.bounds);
    }
}

fn run() {
    let mtm = MainThreadMarker::new().expect("main runs on the main thread");
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.setActivationPolicy(NSApplicationActivationPolicy::Accessory); // no Dock icon
    if !is_trusted(true) {
        eprintln!("Tandem needs Accessibility permission: System Settings > Privacy & Security > Accessibility");
    }
    let _item = ui::menu::install(mtm);
    app::run::start();
    nsapp.run();
}
