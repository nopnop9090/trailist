//! Starting TrayList together with Windows.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` is the per-user autostart
//! list Windows itself acts on: no service, no scheduled task, no elevation, and
//! the entry shows up in Task Manager's startup tab where it can be turned off
//! again. It is written only when the user asks for it, and removed when they ask
//! for that.

use std::path::PathBuf;

use anyhow::Context;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The value name, which is what Windows shows in the startup list.
const NAME: &str = "TrayList";

/// Whether this exact executable is set to start with Windows.
///
/// The stored value is compared against the running executable rather than merely
/// being looked for: an entry left behind by a copy that has since moved is a
/// false "on" that would be confusing to switch off.
pub fn is_enabled() -> bool {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ) else {
        return false;
    };
    let Ok(stored) = key.get_value::<String, _>(NAME) else {
        return false;
    };
    match executable() {
        Some(path) => stored.to_lowercase().contains(&path.to_string_lossy().to_lowercase()),
        // Without a path of our own there is nothing to compare, so an entry
        // under our name is taken at face value.
        None => true,
    }
}

/// Turns the autostart entry on or off.
pub fn set(enabled: bool) -> anyhow::Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN, KEY_READ | KEY_WRITE)
        .context("cannot open the autostart list")?;

    if enabled {
        let path = executable().context("cannot determine the path of the executable")?;
        // Quoted because Windows parses the value as a command line and the
        // folder can contain spaces.
        key.set_value(NAME, &format!("\"{}\"", path.display()))
            .context("cannot write the autostart entry")?;
    } else if key.get_value::<String, _>(NAME).is_ok() {
        key.delete_value(NAME)
            .context("cannot remove the autostart entry")?;
    }

    Ok(())
}

fn executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}