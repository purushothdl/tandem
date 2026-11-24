//! Full screen hides the Dock, and puts it back once no group is full screen.
//! The previous values are written to a state file so `wingroup --restore`
//! can undo it after a crash.

use std::path::PathBuf;
use std::process::Command;

const DOCK_GET: &str = r#"tell application "System Events" to get autohide of dock preferences"#;

pub struct Saved {
    pub dock_autohide: bool,
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

/// Hide the Dock, remembering how it was.
pub fn hide() -> Saved {
    let saved = Saved { dock_autohide: run("osascript", &["-e", DOCK_GET]).as_deref() == Some("true") };
    let path = state_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("dock={}\n", saved.dock_autohide));
    set_dock(true);
    saved
}

pub fn restore(saved: &Saved) {
    set_dock(saved.dock_autohide);
    let _ = std::fs::remove_file(state_file());
}

/// For `wingroup --restore`: undo full screen from the state file.
pub fn restore_after_crash() -> bool {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return false };
    let mut dock = false;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("dock=") {
            dock = v == "true";
        }
    }
    restore(&Saved { dock_autohide: dock });
    true
}
