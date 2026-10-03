//! Looking up the newest GitHub release.
//!
//! Nothing is downloaded. A newer release is announced, and the release page
//! opens only when that question is answered yes. Automatic checks run once
//! when the process starts and again after a day if it is still running:
//! whichever of those comes first.

use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    MESSAGEBOX_STYLE,
};

use crate::i18n::Lang;
use crate::state::AppState;
use crate::version;
use crate::win::launch;

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/nopnop9090/trailist/releases/latest";
const PAGE_PREFIX: &str = "https://github.com/nopnop9090/trailist/";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Asks once, if nobody has answered yet, then checks on the start-or-daily cadence.
pub fn spawn(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("traylist-update".into())
        .spawn(move || {
            let enabled = match app.state::<AppState>().prefs().update_check {
                Some(enabled) => enabled,
                None => {
                    let yes = ask_enable(lang_of(&app));
                    save_choice(&app, yes);
                    yes
                }
            };
            if enabled {
                let _ = look(&app, false);
            }
            loop {
                std::thread::sleep(DAY);
                if app.state::<AppState>().prefs().update_check == Some(true) {
                    let _ = look(&app, false);
                }
            }
        });
}

/// The settings button. An up-to-date copy and a failed lookup are both said out loud.
pub fn check_now(app: &AppHandle) {
    if let Err(error) = look(app, true) {
        crate::trace!("update check: {error}");
    }
}

fn look(app: &AppHandle, report_current: bool) -> anyhow::Result<()> {
    let lang = lang_of(app);
    let release = match latest() {
        Ok(release) => release,
        Err(error) => {
            if report_current {
                tell(&words(lang).failed);
            }
            return Err(error);
        }
    };
    let current = version::version();
    if !newer(&release.tag_name, current) {
        if report_current {
            tell(&words(lang).current.replace("{current}", current));
        }
        return Ok(());
    }
    let question = words(lang)
        .offer
        .replace("{version}", release.tag_name.trim_start_matches('v'))
        .replace("{current}", current);
    if !confirm(&question) {
        return Ok(());
    }
    // The page from the release itself, and only if it is this repository.
    if release.html_url.starts_with(PAGE_PREFIX) {
        if let Err(error) = launch::url(&release.html_url) {
            if report_current {
                tell(&words(lang).failed);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn save_choice(app: &AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    let mut prefs = state.prefs();
    // The settings switch may already have answered while this question was up.
    if prefs.update_check.is_some() {
        return;
    }
    prefs.update_check = Some(enabled);
    if state.store.write(prefs.clone()).is_ok() {
        let _ = app.emit("tl:prefs", &prefs);
    }
}

fn lang_of(app: &AppHandle) -> Lang {
    Lang::resolve(&app.state::<AppState>().prefs().lang)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

fn latest() -> anyhow::Result<Release> {
    let body = fetch()?;
    serde_json::from_str(&body).map_err(|error| anyhow::anyhow!(error))
}

fn fetch() -> anyhow::Result<String> {
    let agent = wide("TrayList");
    let host = wide(HOST);
    let verb = wide("GET");
    let path = wide(PATH);
    unsafe {
        let session = owned(WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        let connection = owned(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0))?;
        let request = owned(WinHttpOpenRequest(
            connection.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available)?;
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut _,
                available,
                &mut read,
            )?;
            chunk.truncate(read as usize);
            body.extend_from_slice(&chunk);
            if body.len() > 256 * 1024 {
                anyhow::bail!("update response is too large");
            }
        }
        String::from_utf8(body).map_err(|error| anyhow::anyhow!(error))
    }
}

struct Guard(*mut std::ffi::c_void);

fn owned(handle: *mut std::ffi::c_void) -> anyhow::Result<Guard> {
    if handle.is_null() {
        Err(anyhow::anyhow!(windows::core::Error::from_thread()))
    } else {
        Ok(Guard(handle))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// `latest` is a release tag (`v0.5.2` or `0.5.2`). A tag that is not three
/// numbers does not count as newer.
fn newer(latest: &str, current: &str) -> bool {
    match (parts(latest), parts(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn parts(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.trim().trim_start_matches('v');
    let mut pieces = version.split('.');
    let major = pieces.next()?.parse().ok()?;
    let minor = pieces.next()?.parse().ok()?;
    let patch = pieces.next()?.parse().ok()?;
    if pieces.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

struct Words {
    ask: &'static str,
    offer: &'static str,
    current: &'static str,
    failed: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::De => Words {
            ask: "Soll TrayList automatisch nach Updates suchen?\n\nGeprüft wird bei jedem Start und, wenn das Programm offen bleibt, einmal am Tag.",
            offer: "Neue Version {version} ist verfügbar (installiert ist {current}).\n\nGitHub-Seite öffnen?",
            current: "TrayList {current} ist die aktuelle Version.",
            failed: "Die Update-Prüfung ist fehlgeschlagen.",
        },
        Lang::En => Words {
            ask: "Should TrayList check for updates automatically?\n\nIt checks on each start and, if it stays open, once a day.",
            offer: "New version {version} is available (this copy is {current}).\n\nOpen the GitHub page?",
            current: "TrayList {current} is the latest version.",
            failed: "The update check failed.",
        },
    }
}

fn ask_enable(lang: Lang) -> bool {
    confirm(words(lang).ask)
}

fn confirm(text: &str) -> bool {
    box_message(text, MB_YESNO | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST) == IDYES
}

fn tell(text: &str) {
    box_message(text, MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST);
}

fn box_message(text: &str, style: MESSAGEBOX_STYLE) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    let body = wide(text);
    let title = wide("TrayList");
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
        )
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn a_higher_patch_is_newer() {
        assert!(newer("v0.5.2", "0.5.1"));
        assert!(!newer("v0.5.1", "0.5.1"));
        assert!(!newer("0.5.0", "0.5.1"));
        assert!(newer("v0.6.0", "0.5.9"));
        assert!(!newer("not-a-version", "0.5.1"));
    }
}
