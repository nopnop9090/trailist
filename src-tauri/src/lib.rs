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

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use parking_lot::Mutex;

use crate::settings::Store;
use crate::state::{AppState, Request, Shared};
use crate::types::Prefs;
use crate::win::{focus, island};

const TRAY_ID: &str = "trailist";
const ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// The language the tray menu is currently labelled in.
///
/// Windows draws that menu, so a language change means handing it a new one. The
/// menu is rebuilt only when this differs from the language asked for, which is
/// what keeps an ordinary settings write from touching the shell at all.
static TRAY_LANG: Mutex<Option<i18n::Lang>> = Mutex::new(None);

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
            commands::hover_time,
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
            commands::system_lang,
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

/// The tray icon and its menu: the whole app is reachable from here, including
/// when the shell decides the chevron is not where the user expects it.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let lang = i18n::Lang::resolve(&app.state::<AppState>().prefs().lang);
    let menu = tray_menu(app, lang)?;
    *TRAY_LANG.lock() = Some(lang);

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

/// The tray menu in one language.
fn tray_menu(app: &AppHandle, lang: i18n::Lang) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let [show, config, quit] = lang.tray_menu();
    let show = MenuItemBuilder::with_id("tl:show", show).build(app)?;
    let config = MenuItemBuilder::with_id("tl:config", config).build(app)?;
    let quit = MenuItemBuilder::with_id("tl:quit", quit).build(app)?;

    MenuBuilder::new(app)
        .items(&[&show, &config])
        .separator()
        .item(&quit)
        .build()
}

/// Re-labels the tray menu when the language setting has moved.
///
/// Called after every settings write, which is cheap: nothing is rebuilt unless the
/// language actually differs from the one on screen.
pub fn retitle_tray(app: &AppHandle, prefs: &Prefs) {
    let lang = i18n::Lang::resolve(&prefs.lang);
    if *TRAY_LANG.lock() == Some(lang) {
        return;
    }
    let Ok(menu) = tray_menu(app, lang) else {
        return;
    };
    if let Some(icon) = app.tray_by_id(TRAY_ID) {
        if icon.set_menu(Some(menu)).is_ok() {
            *TRAY_LANG.lock() = Some(lang);
        }
    }
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