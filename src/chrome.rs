//! "Full mode" hides the Dock and the menu bar, and puts them back afterwards.
//! The previous values are written to a state file so `wingroup --restore`
//! can undo it after a crash.

use std::path::PathBuf;
use std::process::Command;

const DOCK_GET: &str = r#"tell application "System Events" to get autohide of dock preferences"#;

pub struct Saved {
    pub dock_autohide: bool,
    pub menubar_hidden: Option<String>,
}

fn state_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/wingroup/saved.txt")
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn set_dock(hide: bool) {
    let script = format!(r#"tell application "System Events" to set autohide of dock preferences to {hide}"#);
    run("osascript", &["-e", &script]);
}

fn set_menubar(value: Option<&str>) {
    match value {
        Some(v) => {
            let b = if v == "1" || v == "true" { "true" } else { "false" };
            run("defaults", &["write", "NSGlobalDomain", "_HIHideMenuBar", "-bool", b]);
        }
        None => {
            run("defaults", &["delete", "NSGlobalDomain", "_HIHideMenuBar"]);
        }
    }
    run("killall", &["SystemUIServer"]);
}

pub fn enter() -> Saved {
    let saved = Saved {
        dock_autohide: run("osascript", &["-e", DOCK_GET]).as_deref() == Some("true"),
        menubar_hidden: run("defaults", &["read", "NSGlobalDomain", "_HIHideMenuBar"]),
    };
    let path = state_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        &path,
        format!("dock={}\nmenubar={}\n", saved.dock_autohide, saved.menubar_hidden.clone().unwrap_or_else(|| "none".into())),
    );
    set_dock(true);
    set_menubar(Some("1"));
    saved
}

pub fn leave(saved: &Saved) {
    set_dock(saved.dock_autohide);
    set_menubar(saved.menubar_hidden.as_deref());
    let _ = std::fs::remove_file(state_file());
}

/// For `wingroup --restore`: undo full mode from the state file.
pub fn restore_from_file() -> bool {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return false };
    let mut dock = false;
    let mut menubar = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("dock=") {
            dock = v == "true";
        } else if let Some(v) = line.strip_prefix("menubar=") {
            menubar = (v != "none").then(|| v.to_string());
        }
    }
    leave(&Saved { dock_autohide: dock, menubar_hidden: menubar });
    true
}
