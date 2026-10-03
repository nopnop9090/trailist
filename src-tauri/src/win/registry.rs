//! The shell's own record of every tray icon.
//!
//! Since Windows 11 each icon gets a key under
//! `HKCU\Control Panel\NotifyIconSettings`, carrying its executable, its first
//! tooltip and a PNG snapshot of it. `IsPromoted` is the flag that decides
//! whether it sits in the tray or behind the chevron, so writing it is the
//! supported way to say "always show this one".

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const BASE: &str = r"Control Panel\NotifyIconSettings";

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub executable: Option<String>,
    pub tooltip: Option<String>,
    /// `tooltip`, lowercased and whitespace-collapsed, worked out once while
    /// reading the hive.
    pub normalised: String,
    pub promoted: bool,
    pub snapshot: Option<Vec<u8>>,
}

/// Every icon the shell has ever recorded whose application is still installed.
///
/// Explorer never prunes this hive, so it holds leftovers from uninstalled apps.
/// Whether an application still exists is therefore decided here, once, and dead
/// entries are dropped: doing that check inside the per-icon lookup meant tens of
/// thousands of file system calls and made the panel take two seconds to appear.
pub fn entries() -> Vec<Entry> {
    let Ok(base) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(BASE) else {
        return Vec::new();
    };

    base.enum_keys()
        .flatten()
        .filter_map(|key| {
            let sub = base.open_subkey(&key).ok()?;
            let executable: Option<String> = sub.get_value("ExecutablePath").ok();
            if !executable.as_deref().map(executable_exists).unwrap_or(false) {
                return None;
            }

            let tooltip: Option<String> = sub.get_value("InitialTooltip").ok();
            Some(Entry {
                normalised: tooltip.as_deref().map(normalise).unwrap_or_default(),
                executable,
                tooltip,
                promoted: sub
                    .get_value::<u32, _>("IsPromoted")
                    .map(|value| value == 1)
                    .unwrap_or(false),
                snapshot: sub
                    .get_raw_value("IconSnapshot")
                    .ok()
                    .map(|value| value.bytes.into_owned()),
                key,
            })
        })
        .collect()
}

/// Finds the entry among `entries` whose recorded tooltip looks most like the
/// tooltip UI Automation just gave us.
///
/// The two are rarely identical — the tooltip changes with the app's state while
/// `InitialTooltip` was captured the first time the icon appeared — so this is a
/// prefix vote rather than an equality test. Entries are passed in because the
/// hive is read once per flyout opening, not once per icon.
pub fn best_match(entries: &[Entry], tooltip: &str) -> Option<Entry> {
    let needle = normalise(tooltip);
    if needle.chars().count() < 3 {
        return None;
    }

    entries
        .iter()
        .map(|entry| (shared_prefix(&needle, &entry.normalised), entry))
        .filter(|(score, _)| *score >= 3)
        .max_by_key(|(score, _)| *score)
        .map(|(_, entry)| entry.clone())
}

/// Turns a tray icon on or off in the visible part of the tray.
///
/// Explorer picks the change up on its own; it does not need a restart.
pub fn set_promoted(key: &str, promoted: bool) -> anyhow::Result<()> {
    let path = format!(r"{BASE}\{key}");
    let sub = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(&path, KEY_READ | KEY_WRITE)
        .map_err(|error| anyhow::anyhow!("cannot open {path}: {error}"))?;
    let value: u32 = u32::from(promoted);
    sub.set_value("IsPromoted", &value)
        .map_err(|error| anyhow::anyhow!("cannot write IsPromoted on {path}: {error}"))?;
    Ok(())
}

fn executable_exists(path: &str) -> bool {
    // Ids in this hive are stored with a known-folder GUID in front of the real
    // path, so only the tail after the GUID is a usable file path.
    let trimmed = path
        .split_once('\\')
        .map(|(head, tail)| {
            if head.starts_with('{') && head.ends_with('}') {
                tail.to_string()
            } else {
                path.to_string()
            }
        })
        .unwrap_or_else(|| path.to_string());
    let candidate = std::path::Path::new(&trimmed);
    candidate.exists()
        || std::path::Path::new(&format!("C:\\Program Files\\{trimmed}")).exists()
        || std::path::Path::new(&format!("C:\\Program Files (x86)\\{trimmed}")).exists()
}

fn normalise(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn shared_prefix(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .count()
}