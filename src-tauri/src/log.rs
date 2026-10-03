//! Optional tracing.
//!
//! The interesting failures here are timing problems between our window and the
//! shell's flyout, which are impossible to reason about from the outside. Setting
//! `TRAILIST_DEBUG=1` makes the watcher narrate what it sees and does.
//!
//! The app is built as a GUI program, so there is no console for that narration
//! to land in; it is appended to `logs/trailist.log` next to the source tree,
//! which a debug session can also tail.

use std::io::Write;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("TRAILIST_DEBUG").is_some())
}

/// Whether tracing is on, so callers can skip work that only exists for a debug
/// session.
pub fn is_enabled() -> bool {
    enabled()
}

/// Where a debug session may drop files, if anywhere sensible exists.
pub fn directory() -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub fn debug(message: impl AsRef<str>) {
    if !enabled() {
        return;
    }
    // Harmless when there is no console, and captured when the app happens to be
    // started from one.
    eprintln!("[trailist] {}", message.as_ref());

    if let Some(dir) = directory() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("trailist.log"))
        {
            let _ = writeln!(file, "[trailist] {}", message.as_ref());
        }
    }
}

/// `format!`-style version, so call sites can stay readable.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::log::debug(format!($($arg)*))
    };
}