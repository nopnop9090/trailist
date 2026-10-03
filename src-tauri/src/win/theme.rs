//! Which colour scheme the shell is drawing in.
//!
//! The panel stands in for the tray flyout, so it should look like the shell it
//! replaces rather than like a preference of its own. Windows keeps the two
//! schemes side by side in the registry, and `SystemUsesLightTheme` is the one
//! the taskbar and the flyout follow.

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// Whether the shell is currently drawing light chrome.
///
/// Windows separates the *app* scheme (`AppsUseLightTheme`, which is what
/// `prefers-color-scheme` in a webview reports) from the *shell* scheme, and the
/// flyout belongs to the shell. Anything unreadable counts as light, which is
/// also what Windows uses on a fresh profile.
pub fn is_light() -> bool {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(PERSONALIZE, KEY_READ)
    else {
        return true;
    };
    key.get_value::<u32, _>("SystemUsesLightTheme")
        .map(|value| value == 1)
        .unwrap_or(true)
}

/// Whether the panel should paint its dark palette.
///
/// Read on every flyout opening rather than watched: it is one registry read, and
/// the opening is the only moment the answer is needed.
pub fn is_dark() -> bool {
    !is_light()
}