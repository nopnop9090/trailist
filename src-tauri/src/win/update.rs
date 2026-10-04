//! Looking up the newest GitHub release.
//!
//! A newer release offers to download that version's setup and start it, and,
//! as the other choice, to open the release page. Nothing starts until one of
//! those answers is given. Automatic checks run once when the process starts
//! and again after a day if it is still running: whichever of those comes first.

use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDNO, IDYES, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    MB_YESNO, MB_YESNOCANCEL, MESSAGEBOX_STYLE, SW_SHOWNORMAL,
};

use crate::i18n::Lang;
use crate::state::AppState;
use crate::version;
use crate::win::launch;

const LATEST_URL: &str = "https://api.github.com/repos/nopnop9090/trailist/releases/latest";
const PAGE_PREFIX: &str = "https://github.com/nopnop9090/trailist/";
const DOWNLOAD_PREFIX: &str = "https://github.com/nopnop9090/trailist/releases/download/";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// The setup is about one and a half megabytes. Past this it is not our installer.
const SETUP_LIMIT: usize = 32 * 1024 * 1024;

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
    let version = release.tag_name.trim().trim_start_matches('v');
    let question = words(lang)
        .offer
        .replace("{version}", version)
        .replace("{current}", current);
    match choose(&question) {
        Choice::Later => Ok(()),
        Choice::Page => open_page(&release, lang, report_current),
        Choice::Install => match install_release(&release, version) {
            Ok(()) => Ok(()),
            Err(error) => {
                tell(&words(lang).download_failed);
                if confirm(&words(lang).page) {
                    open_page(&release, lang, report_current)?;
                }
                Err(error)
            }
        },
    }
}

fn open_page(release: &Release, lang: Lang, report_current: bool) -> anyhow::Result<()> {
    if !release.html_url.starts_with(PAGE_PREFIX) {
        return Ok(());
    }
    if let Err(error) = launch::url(&release.html_url) {
        if report_current {
            tell(&words(lang).failed);
        }
        return Err(error);
    }
    Ok(())
}

fn install_release(release: &Release, version: &str) -> anyhow::Result<()> {
    let asset = setup_asset(&release.assets, version)
        .ok_or_else(|| anyhow::anyhow!("release has no setup"))?;
    let bytes = fetch_https(&asset.browser_download_url, SETUP_LIMIT)?;
    anyhow::ensure!(bytes.starts_with(b"MZ"), "setup is not a program");
    let path = std::env::temp_dir().join(&asset.name);
    std::fs::write(&path, &bytes)?;
    launch_setup(&path)
}

fn launch_setup(path: &std::path::Path) -> anyhow::Result<()> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    anyhow::ensure!(
        path.starts_with(std::env::temp_dir())
            && name.to_ascii_lowercase().starts_with("traylist_")
            && name.to_ascii_lowercase().ends_with("_x64-setup.exe")
            && !name.contains(['/', '\\']),
        "setup path was refused"
    );
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!((result.0 as isize) > 32, "the setup could not be started");
    Ok(())
}

/// The installer published with this release, and only that file.
fn setup_asset<'a>(assets: &'a [Asset], version: &str) -> Option<&'a Asset> {
    assets.iter().find(|asset| {
        is_setup_name(&asset.name, version)
            && asset.browser_download_url.starts_with(DOWNLOAD_PREFIX)
            && !asset.browser_download_url.contains("..")
    })
}

fn is_setup_name(name: &str, version: &str) -> bool {
    if version.is_empty() || version.chars().any(|ch| !(ch.is_ascii_digit() || ch == '.')) {
        return false;
    }
    name.eq_ignore_ascii_case(&format!("TrayList_{version}_x64-setup.exe"))
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
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn latest() -> anyhow::Result<Release> {
    let body = fetch_https(LATEST_URL, 256 * 1024)?;
    let body = String::from_utf8(body).map_err(|error| anyhow::anyhow!(error))?;
    serde_json::from_str(&body).map_err(|error| anyhow::anyhow!(error))
}

fn fetch_https(url: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
    let (host, path) = https_parts(url).ok_or_else(|| anyhow::anyhow!("update url was refused"))?;
    let agent = wide("TrayList");
    let host = wide(host);
    let verb = wide("GET");
    let path = wide(&path);
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
            if body.len() > limit {
                anyhow::bail!("update response is too large");
            }
        }
        Ok(body)
    }
}

/// `https://host/path`, limited to the two GitHub hosts this check talks to.
fn https_parts(url: &str) -> Option<(&str, String)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !host.eq_ignore_ascii_case("api.github.com") && !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    if path.contains("..") || path.contains('\\') {
        return None;
    }
    Some((host, format!("/{path}")))
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
    page: &'static str,
    current: &'static str,
    failed: &'static str,
    download_failed: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::De => Words {
            ask: "Soll TrayList automatisch nach Updates suchen?\n\nGeprüft wird bei jedem Start und, wenn das Programm offen bleibt, einmal am Tag.",
            offer: "Neue Version {version} ist verfügbar (installiert ist {current}).\n\nJa — Setup herunterladen und Installation starten\nNein — GitHub-Seite öffnen",
            page: "GitHub-Seite öffnen?",
            current: "TrayList {current} ist die aktuelle Version.",
            failed: "Die Update-Prüfung ist fehlgeschlagen.",
            download_failed: "Das Setup konnte nicht heruntergeladen werden.",
        },
        Lang::En => Words {
            ask: "Should TrayList check for updates automatically?\n\nIt checks on each start and, if it stays open, once a day.",
            offer: "New version {version} is available (this copy is {current}).\n\nYes — download the setup and start installation\nNo — open the GitHub page",
            page: "Open the GitHub page?",
            current: "TrayList {current} is the latest version.",
            failed: "The update check failed.",
            download_failed: "The setup could not be downloaded.",
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Install,
    Page,
    Later,
}

fn choose(text: &str) -> Choice {
    match box_message(
        text,
        MB_YESNOCANCEL | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
    ) {
        IDYES => Choice::Install,
        IDNO => Choice::Page,
        _ => Choice::Later,
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
    use super::{https_parts, is_setup_name, newer, setup_asset, Asset};

    #[test]
    fn a_higher_patch_is_newer() {
        assert!(newer("v0.5.2", "0.5.1"));
        assert!(!newer("v0.5.1", "0.5.1"));
        assert!(!newer("0.5.0", "0.5.1"));
        assert!(newer("v0.6.0", "0.5.9"));
        assert!(!newer("not-a-version", "0.5.1"));
    }

    #[test]
    fn only_this_releases_setup_is_accepted() {
        let assets = vec![
            Asset {
                name: "TrayList.exe".into(),
                browser_download_url: "https://github.com/nopnop9090/trailist/releases/download/v0.5.3/TrayList.exe".into(),
            },
            Asset {
                name: "TrayList_0.5.3_x64-setup.exe".into(),
                browser_download_url: "https://example.com/TrayList_0.5.3_x64-setup.exe".into(),
            },
            Asset {
                name: "TrayList_0.5.3_x64-setup.exe".into(),
                browser_download_url: "https://github.com/nopnop9090/trailist/releases/download/v0.5.3/TrayList_0.5.3_x64-setup.exe".into(),
            },
        ];
        let chosen = setup_asset(&assets, "0.5.3").unwrap();
        assert!(chosen.browser_download_url.starts_with("https://github.com/nopnop9090/trailist/"));
        assert!(is_setup_name(&chosen.name, "0.5.3"));
        assert!(setup_asset(&assets, "0.5.4").is_none());
        assert!(https_parts("https://evil.example/setup.exe").is_none());
        assert!(https_parts("https://github.com/nopnop9090/trailist/releases/download/v0.5.3/../other").is_none());
    }
}
