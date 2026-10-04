//! Opening a link in whatever the user has set as their browser.
//!
//! `ShellExecuteW` is the same call Windows makes when a link is clicked, so the
//! result honours the default-browser setting, and because it goes through the
//! shell there is no console window flashing up the way a spawned `cmd` would.

use anyhow::bail;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Opens an `https` address in the default browser.
///
/// The scheme is checked here rather than at the call site, so that this can never
/// turn into "start any local program" reachable from the panel.
pub fn url(target: &str) -> anyhow::Result<()> {
    if !target.starts_with("https://") {
        bail!("nur https-Adressen lassen sich oeffnen");
    }

    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // The shell reports success as a value above 32; everything below is an error
    // code from a long list of small integers.
    anyhow::ensure!(
        (result.0 as isize) > 32,
        "die Shell konnte {target} nicht oeffnen"
    );
    Ok(())
}

/// GUID SecurityHealthSystray registers for the Windows Security icon.
const WINDOWS_SECURITY: &str = "guid:{BA82E2DC-F405-47AD-B032-CF0FAA0E3933}";

/// That icon is added once, usually before the host is attached, and later
/// messages only change the tooltip. The callback stays unknown, so there is
/// nothing to forward. A click with a real callback is left to the app.
pub fn is_windows_security(key: &str, callback: u32) -> bool {
    callback == 0 && key.eq_ignore_ascii_case(WINDOWS_SECURITY)
}

/// Opens Windows Security, which is what a click on that icon does.
pub fn windows_security() -> anyhow::Result<()> {
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("windowsdefender:"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(
        (result.0 as isize) > 32,
        "Windows Security liess sich nicht oeffnen"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_security_icon_without_a_callback_is_opened_directly() {
        assert!(is_windows_security(
            "guid:{BA82E2DC-F405-47AD-B032-CF0FAA0E3933}",
            0
        ));
        assert!(!is_windows_security(
            "guid:{BA82E2DC-F405-47AD-B032-CF0FAA0E3933}",
            1024
        ));
        assert!(!is_windows_security(
            "guid:{7820AE78-23E3-4229-82C1-E41CB67D5B9C}",
            0
        ));
    }
}