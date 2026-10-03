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