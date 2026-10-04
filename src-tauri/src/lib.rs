//! TrayList: the hidden Windows tray icons as a scrollable list with names,
//! instead of an icon grid where every entry has to be recognised by sight.

pub mod commands;
pub mod i18n;
pub mod log;
pub mod overlay;
pub mod settings;
pub mod state;
pub mod types;
pub mod version;
pub mod watcher;
pub mod win;

use std::sync::mpsc::channel;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::Store;
use crate::state::{AppState, Request, Shared};
use crate::win::{focus, island};

pub fn run() {
    let store = Arc::new(Store::open());
    let shared = Arc::new(Shared::default());
    let (sender, receiver) = channel();

    let watcher_shared = shared.clone();
    let watcher_store = store.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            shared: shared.clone(),
            store,
        })
        .invoke_handler(tauri::generate_handler![
            commands::activate,
            commands::hover,
            commands::double_click_time,
            commands::hover_time,
            commands::about,
            commands::check_for_update,
            commands::dismiss,
            commands::current,
            commands::dark_theme,
            commands::get_autostart,
            commands::get_prefs,
            commands::open_url,
            commands::set_autostart,
            commands::set_prefs,
            commands::set_pinned,
            commands::system_lang,
            commands::open_config,
            commands::quit,
            commands::toggle,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            shared.attach(sender);

            // One running copy. A second launch asks this one to show the list
            // and then leaves.
            if !win::instance::claim(handle.clone()) {
                handle.exit(0);
                return Ok(());
            }
            win::update::spawn(handle.clone());
            register_hotkey(&handle, &watcher_store);
            watcher::spawn(handle, watcher_shared, receiver);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != overlay::WINDOW {
                return;
            }
            match event {
                // Closing is not offered; the window is hidden instead.
                WindowEvent::CloseRequested { api, .. } => api.prevent_close(),
                WindowEvent::Focused(true) => overlay::note_focus_gained(),
                // Clicking anywhere else blurs us, which is the dismissal the
                // user expects from a tray flyout. Two blurs are not that: one
                // that arrives while the panel is still settling, and one that
                // goes to the taskbar itself, because that is the user reaching
                // for the chevron and the tray toggle path already handles it.
                WindowEvent::Focused(false) => {
                    let app = window.app_handle();
                    if app.state::<AppState>().shared.menu_hold() {
                        trace!("overlay: blur ignored, a row menu is using the foreground");
                    } else if !overlay::is_dismissable() {
                        trace!("overlay: blur ignored, still settling");
                    } else if island::is_shell_surface(focus::foreground()) {
                        trace!("overlay: blur went to the taskbar, letting the chevron decide");
                    } else {
                        trace!("overlay: lost the foreground, dismissing");
                        overlay::hide(app);
                        app.state::<AppState>().shared.set_visible(false);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("TrayList konnte nicht starten")
        .run(|app, event| {
            // A tray app has no windows on screen most of the time, so the
            // default "everything closed, time to quit" must not apply.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            let _ = app;
        });
}

/// Registers the "show the list" hotkey, if one is configured.
///
/// A hotkey that is already taken is reported and otherwise ignored: the panel
/// still opens from the chevron, so a clash is not worth failing over.
fn register_hotkey(app: &AppHandle, store: &Store) {
    let Some(accelerator) = store.prefs().hotkey else {
        return;
    };
    let Ok(shortcut) = accelerator.parse::<Shortcut>() else {
        eprintln!("TrayList: '{accelerator}' ist keine gueltige Tastenkombination.");
        return;
    };

    let result = app.global_shortcut().on_shortcut(
        shortcut,
        move |app: &AppHandle, _shortcut: &Shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                app.state::<AppState>().shared.send(Request::Toggle);
            }
        },
    );
    if let Err(error) = result {
        eprintln!("TrayList: Tastenkombination '{accelerator}' ist belegt ({error}).");
    }
}

/// Lets the commands push a fresh list without going through the watcher.
pub trait EmitList {
    fn emit_list(&self, items: Vec<types::TrayItem>) -> tauri::Result<()>;
}

impl EmitList for AppHandle {
    fn emit_list(&self, items: Vec<types::TrayItem>) -> tauri::Result<()> {
        self.emit(
            "tl:list",
            types::TrayList {
                items,
                error: None,
                source: watcher::shell_build(),
                // The same list, re-sent: nothing was read, and the panel must not
                // treat it as an opening.
                opening: false,
                direct: self.try_state::<AppState>().map(|state| state.shared.direct()).unwrap_or(false),
            },
        )
    }
}