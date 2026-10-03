//! A line log in the user's temp directory.
//!
//! The host runs inside explorer, which has no console. The probe and TrayList
//! both read the same file. Writing is best-effort: a full disk must not take
//! the shell down, and the window procedure never waits on the file.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn path() -> std::path::PathBuf {
    let dir = std::env::var_os("TEMP").unwrap_or_else(|| "C:\\Windows\\Temp".into());
    std::path::PathBuf::from(dir).join("trailist-host.log")
}

/// Spy mode is a file the probe creates before it loads the DLL. Explorer does
/// not inherit the probe's environment, so a file is the signal.
pub fn spy_path() -> std::path::PathBuf {
    let dir = std::env::var_os("TEMP").unwrap_or_else(|| "C:\\Windows\\Temp".into());
    std::path::PathBuf::from(dir).join("trailist-host-spy")
}

pub fn line(text: &str) {
    let Ok(mut slot) = LOG.lock() else {
        return;
    };
    if slot.is_none() {
        *slot = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path())
            .ok();
    }
    if let Some(file) = slot.as_mut() {
        let _ = writeln!(file, "{text}");
        let _ = file.flush();
    }
}

pub fn hex(prefix: &str, bytes: &[u8]) {
    const MAX: usize = 128;
    let shown = bytes.len().min(MAX);
    let mut encoded = String::with_capacity(shown * 3);
    for (index, byte) in bytes[..shown].iter().enumerate() {
        if index > 0 {
            encoded.push(' ');
        }
        encoded.push_str(&format!("{byte:02X}"));
    }
    if bytes.len() > MAX {
        encoded.push_str(" …");
    }
    line(&format!("{prefix} ({}) {encoded}", bytes.len()));
}

/// `DLL_PROCESS_ATTACH`, named so the entry point reads as a reason.
pub const ATTACH: u32 = DLL_PROCESS_ATTACH;
