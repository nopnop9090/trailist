//! The IPC surface the panel talks to.
//!
//! Nothing here touches the shell directly: clicks and the chevron are handed to
//! the watcher thread, which is the only thread with UI Automation.

use tauri::{AppHandle, State};

use crate::overlay;
use crate::state::{AppState, Request};
use crate::types::{About, Prefs, TrayList};
use crate::win::registry;
use crate::EmitList;

/// Replays a click on one of the listed icons.
///
/// The panel is deliberately not touched here. Only the watcher thread decides
/// when it goes away, and it has to happen before the replayed click, because our
/// always-on-top window would otherwise swallow it. Hiding it from here as well
/// would also clear the panel-open flag while the watcher is still on `Open`,
/// which makes the watcher move to idle and drop the click entirely.
#[tauri::command]
pub fn activate(
    state: State<'_, AppState>,
    index: usize,
    button: String,
) -> Result<(), String> {
    let item = state
        .shared
        .item_at(index)
        .ok_or_else(|| "Dieser Eintrag existiert nicht mehr.".to_string())?;

    let sent = state.shared.send(Request::Click {
        x: item.x,
        y: item.y,
        right: button == "right",
    });
    if !sent {
        return Err("Der Hintergrunddienst laeuft nicht.".to_string());
    }
    Ok(())
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
pub fn set_prefs(app: AppHandle, state: State<'_, AppState>, prefs: Prefs) -> Result<Prefs, String> {
    state
        .store
        .write(prefs.clone())
        .map_err(|error| error.to_string())?;

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
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    crate::win::autostart::set(enabled).map_err(|error| error.to_string())?;
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
pub fn open_url(url: String) -> Result<(), String> {
    crate::win::launch::url(&url).map_err(|error| error.to_string())
}

/// Turns an icon on or off in the visible part of the tray, which is what the
/// shell's own "IsPromoted" flag means.
#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
    pinned: bool,
) -> Result<(), String> {
    let item = state
        .shared
        .item_at(index)
        .ok_or_else(|| "Dieser Eintrag existiert nicht mehr.".to_string())?;
    let key = item
        .registry_key
        .ok_or_else(|| "Fuer dieses Symbol wurde kein Eintrag in der Registry gefunden.".to_string())?;

    registry::set_promoted(&key, pinned).map_err(|error| error.to_string())?;
    // Keep the panel's own idea of the icon honest.
    let mut items = state.shared.items();
    if let Some(target) = items.iter_mut().find(|candidate| candidate.index == index) {
        target.promoted = pinned;
    }
    if state.prefs().pinned_first {
        crate::watcher::apply_order(&mut items, &state.prefs());
    }
    let _ = app.emit_list(items);
    Ok(())
}

/// Opens the folder the settings file lives in.
#[tauri::command]
pub fn open_config(state: State<'_, AppState>) -> Result<(), String> {
    let directory = state.store.directory().to_path_buf();
    std::process::Command::new("explorer")
        .arg(&directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Opens or closes the list without touching the chevron. Used by the hotkey and
/// the tray menu.
#[tauri::command]
pub fn toggle(state: State<'_, AppState>) {
    state.shared.send(Request::Toggle);
}