//! ~/Library/Logs/wingroup.log

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// Failing to log is not worth stopping for.
pub fn log(msg: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(home).join("Library/Logs/wingroup.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}
