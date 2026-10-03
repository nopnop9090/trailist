//! TrayList: the hidden Windows tray icons as a scrollable list with names,
//! instead of an icon grid where every entry has to be recognised by sight.

pub mod commands;
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

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::Store;
use crate::state::{AppState, Request, Shared};
use crate::win::{focus, island};

const TRAY_ID: &str = "trailist";
const ICON: &[u8] = include_bytes!("../icons/32x32.png");

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
            commands::about,
            commands::dismiss,
            commands::current,
            commands::dark_theme,
            commands::get_autostart,
            commands::get_prefs,
            commands::open_url,
            commands::set_autostart,
            commands::set_prefs,
            commands::set_pinned,
            commands::open_config,
            commands::toggle,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            shared.attach(sender);

            build_tray(&handle)?;
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
                    if !overlay::is_dismissable() {
                        trace!("overlay: blur ignored, still settling");
                    } else if island::is_shell_surface(focus::foreground()) {
                        trace!("overlay: blur went to the taskbar, letting the chevron decide");
                    } else {
                        trace!("overlay: lost the foreground, dismissing");
                        let app = window.app_handle();
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

/// The tray icon and its menu: the whole app is reachable from here, including
/// when the shell decides the chevron is not where the user expects it.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id("tl:show", "Symbol-Liste zeigen").build(app)?;
    let config = MenuItemBuilder::with_id("tl:config", "Einstellungen oeffnen").build(app)?;
    let quit = MenuItemBuilder::with_id("tl:quit", "Beenden").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &config])
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::image::Image::from_bytes(ICON)?)
        .tooltip("TrayList")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "tl:show" => {
                app.state::<AppState>().shared.send(Request::Toggle);
            }
            "tl:config" => {
                let directory = app.state::<AppState>().store.directory().to_path_buf();
                let _ = std::process::Command::new("explorer").arg(directory).spawn();
            }
            "tl:quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
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
            },
        )
    }
}