//! The IPC surface the panel talks to.
//!
//! Nothing here touches the shell directly: clicks and the chevron are handed to
//! the watcher thread, which is the only thread with UI Automation.

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::overlay;
use crate::state::{AppState, Request};
use crate::types::{About, Fault, Prefs, TrayList};
use crate::win::{host, registry};
use crate::EmitList;

/// A row rectangle in CSS pixels, plus the webview's device-pixel ratio.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowBox {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub dpr: f64,
}

fn row_on_screen(app: &AppHandle, row: RowBox) -> host::ScreenRect {
    let hwnd = overlay::window(app)
        .and_then(|window| window.hwnd().ok())
        .map(|handle| handle.0 as isize);
    host::screen_rect(hwnd, row.left, row.top, row.right, row.bottom, row.dpr)
}

/// Replays a click on one of the listed icons.
///
/// The panel is deliberately not touched here. Only the watcher thread decides
/// when it goes away, and it has to happen before the replayed click, because our
/// always-on-top window would otherwise swallow it. Hiding it from here as well
/// would also clear the panel-open flag while the watcher is still on `Open`,
/// which makes the watcher move to idle and drop the click entirely.
#[tauri::command]
pub fn activate(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    button: String,
    row: RowBox,
) -> Result<(), Fault> {
    let _ = state
        .shared
        .item_at(index)
        .ok_or_else(|| Fault::new("item_gone"))?;

    let sent = state.shared.send(Request::Click {
        index,
        right: button == "right",
        double_click: button == "double",
        // The first half of a mouse double-click. Keyboard Enter stays "left"
        // and closes immediately.
        keep: button == "arm",
        anchor: row_on_screen(&app, row),
    });
    if !sent {
        return Err(Fault::new("worker_down"));
    }
    Ok(())
}

/// Tells the owning app the pointer entered or left this row.
///
/// The panel stays up. Leaving cancels a hover the app already opened.
#[tauri::command]
pub fn hover(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    enter: bool,
    row: RowBox,
) -> Result<(), Fault> {
    let sent = state.shared.send(Request::Hover {
        index,
        enter,
        anchor: row_on_screen(&app, row),
    });
    if !sent {
        return Err(Fault::new("worker_down"));
    }
    Ok(())
}

/// `GetDoubleClickTime`, so a second click still counts as a double-click.
#[tauri::command]
pub fn double_click_time() -> u32 {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }.clamp(100, 2_000)
}

/// `SPI_GETMOUSEHOVERTIME`, so a row waits as long as the rest of the desktop.
#[tauri::command]
pub fn hover_time() -> u32 {
    let mut millis = 400u32;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETMOUSEHOVERTIME,
            0,
            Some(&mut millis as *mut u32 as *mut _),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    millis.clamp(50, 2_000)
}

/// Puts the panel away without touching any icon.
#[tauri::command]
pub fn dismiss(app: AppHandle, state: State<'_, AppState>) {
    overlay::hide(&app);
    state.shared.set_visible(false);
}

/// The list as it stands, for a window that was opened after the fact.
#[tauri::command]
pub fn current(state: State<'_, AppState>) -> TrayList {
    TrayList {
        items: state.shared.items(),
        error: None,
        source: crate::watcher::shell_build(),
        // Nothing was read just now, so this is not an opening.
        opening: false,
        direct: state.shared.direct(),
    }
}

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> Prefs {
    state.prefs()
}

/// Whether the shell is currently dark, so the panel can follow it.
///
/// Asked for explicitly instead of using `prefers-color-scheme`, which reports
/// the webview's *app* theme and can differ from the shell scheme the tray flyout
/// — and therefore the panel — is drawn in.
#[tauri::command]
pub fn dark_theme() -> bool {
    crate::win::theme::is_dark()
}

/// Stores the preferences and re-orders the list that is already on screen, so a
/// change to the sort order is visible immediately.
///
/// The panel is re-placed as well, which is what makes the height limit and the
/// gap to the taskbar feel like live controls rather than settings that only count
/// next time.
#[tauri::command]
pub fn set_prefs(app: AppHandle, state: State<'_, AppState>, prefs: Prefs) -> Result<Prefs, Fault> {
    state
        .store
        .write(prefs.clone())
        .map_err(|error| Fault::with("settings_write", error))?;

    // The tray menu is drawn by Windows, not by us, so a language change has to be
    // handed to the shell as a new menu rather than merely taken note of.
    crate::retitle_tray(&app, &prefs);

    let mut items = state.shared.items();
    crate::watcher::apply_order(&mut items, &prefs);
    let _ = app.emit_list(items);

    if let Some(flyout) = state.shared.flyout() {
        let count = state.shared.items().len();
        overlay::relayout(&app, flyout, count, &prefs);
    }

    Ok(prefs)
}

/// Whether TrayList is set to start with Windows.
#[tauri::command]
pub fn get_autostart() -> bool {
    crate::win::autostart::is_enabled()
}

/// Turns the autostart entry on or off and reports what it ended up as.
///
/// The result is read back rather than assumed, so a registry that refused the
/// write shows up as the check box staying where it was.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, Fault> {
    crate::win::autostart::set(enabled).map_err(|error| Fault::with("autostart_write", error))?;
    Ok(crate::win::autostart::is_enabled())
}

/// Version, build stamp and shell build, for the panel's footer.
#[tauri::command]
pub fn about() -> About {
    About {
        version: crate::version::version().to_string(),
        built: crate::version::built().to_string(),
        shell: crate::watcher::shell_build(),
    }
}

/// Opens a link in the user's own browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), Fault> {
    crate::win::launch::url(&url).map_err(|error| Fault::with("open_url", error))
}

/// Turns an icon on or off in the visible part of the tray, which is what the
/// shell's own "IsPromoted" flag means.
#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    pinned: bool,
) -> Result<(), Fault> {
    let item = state
        .shared
        .item_at(index)
        .ok_or_else(|| Fault::new("item_gone"))?;
    let key = item
        .registry_key
        .ok_or_else(|| Fault::new("no_registry_entry"))?;

    registry::set_promoted(&key, pinned).map_err(|error| Fault::with("registry_write", error))?;
    // Keep the panel's own idea of the icon honest.
    let mut items = state.shared.items();
    if let Some(target) = items.iter_mut().find(|candidate| candidate.index == index) {
        target.promoted = pinned;
    }
    if let Some(flyout) = state.shared.flyout() {
        state
            .shared
            .set_items(items.clone(), state.shared.island().unwrap_or(0), flyout);
    }
    if state.prefs().pinned_first {
        crate::watcher::apply_order(&mut items, &state.prefs());
    }
    let _ = app.emit_list(items);
    Ok(())
}

/// Opens the folder the settings file lives in.
#[tauri::command]
pub fn open_config(state: State<'_, AppState>) -> Result<(), Fault> {
    let directory = state.store.directory().to_path_buf();
    std::process::Command::new("explorer")
        .arg(&directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| Fault::with("open_config", error))
}

/// Opens or closes the list without touching the chevron. Used by the hotkey and
/// the tray menu.
#[tauri::command]
pub fn toggle(state: State<'_, AppState>) {
    state.shared.send(Request::Toggle);
}

/// The language Windows' own interface is in, as `de` or `en`.
///
/// Asked for once at startup, because a `system` setting has to be resolved by
/// whoever shows the words: the panel needs it for its own text, and Rust needs it
/// for the tray menu.
#[tauri::command]
pub fn system_lang() -> String {
    crate::i18n::Lang::resolve("system").tag().to_string()
}