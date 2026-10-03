//! The window the installer uses to find this process and ask it to leave.
//!
//! The executable is not always named `TrayList.exe`: a debug build is
//! `trailist.exe`, and an installed copy takes the product name. Killing by
//! those file names would also stop some other program that happened to use
//! one of them. This window is the identity instead. The class and title are
//! private, `FindWindow` can see it (a message-only window cannot), and it
//! answers one registered message by leaving. `WM_USER` is not that message:
//! those ids belong to whatever window class receives them, so a broadcast
//! would be interpreted by unrelated windows.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, RegisterClassW,
    RegisterWindowMessageW, SendMessageTimeoutW, TranslateMessage, MSG, SMTO_ABORTIFHUNG,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use tauri::{AppHandle, Manager};

use crate::state::{AppState, Request};

/// Agreed with `windows/hooks.nsh`. Changing either side without the other
/// makes the installer miss the running copy.
const CLASS: &str = "TrayList.Instance";
const TITLE: &str = "TrayList";
const SHUTDOWN_NAME: &str = "TrayList.Shutdown";
const SHOW_NAME: &str = "TrayList.Show";
/// `wParam` on both messages. Anything else is ignored.
const MAGIC: usize = 0x5459_4C31;
const SINGLE_INSTANCE: &str = "Local\\TrayList.SingleInstance";

static APP: OnceLock<AppHandle> = OnceLock::new();
static SHUTDOWN: AtomicU32 = AtomicU32::new(0);
static SHOW: AtomicU32 = AtomicU32::new(0);

/// Takes the single-instance lock and opens the handshake window.
///
/// A second process finds the lock already held, asks the running copy to
/// show the list, and is told to leave. The mutex handle is left open for
/// the life of the process; closing it would drop the lock.
pub fn claim(app: AppHandle) -> bool {
    let name = wide(SINGLE_INSTANCE);
    let handle = match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
        Ok(handle) => handle,
        Err(error) => {
            crate::trace!("instance lock: {error}");
            start(app);
            return true;
        }
    };
    let taken = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if taken {
        let _ = unsafe { CloseHandle(handle) };
        ask_running_copy_to_show();
        return false;
    }
    // `HANDLE` is not closed on drop. Leaving it open holds the lock until exit.
    let _lock = handle;
    start(app);
    true
}

fn ask_running_copy_to_show() {
    let show_name = wide(SHOW_NAME);
    let show = unsafe { RegisterWindowMessageW(PCWSTR(show_name.as_ptr())) };
    if show == 0 {
        return;
    }
    let class = wide(CLASS);
    let title = wide(TITLE);
    for _ in 0..20 {
        let hwnd = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) };
        let Ok(hwnd) = hwnd else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        if hwnd.is_invalid() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut replied = 0usize;
        let _ = unsafe {
            SendMessageTimeoutW(
                hwnd,
                show,
                WPARAM(MAGIC),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                1000,
                Some(&mut replied),
            )
        };
        return;
    }
}

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("traylist-instance".into())
        .spawn(move || {
            if let Err(error) = run(app) {
                crate::trace!("instance window: {error}");
            }
        });
}

fn run(app: AppHandle) -> windows::core::Result<()> {
    let _ = APP.set(app);
    let shutdown_name = wide(SHUTDOWN_NAME);
    let shutdown = unsafe { RegisterWindowMessageW(PCWSTR(shutdown_name.as_ptr())) };
    if shutdown == 0 {
        return Err(windows::core::Error::from_thread());
    }
    SHUTDOWN.store(shutdown, Ordering::Release);
    let show_name = wide(SHOW_NAME);
    let show = unsafe { RegisterWindowMessageW(PCWSTR(show_name.as_ptr())) };
    if show != 0 {
        SHOW.store(show, Ordering::Release);
    }

    let class = wide(CLASS);
    let title = wide(TITLE);
    let instance = unsafe { GetModuleHandleW(PCWSTR::null())? };
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(windows::core::Error::from_thread());
    }

    // Hidden, unowned and out of the taskbar. `FindWindow` still sees it,
    // which a `HWND_MESSAGE` window would not.
    let handle = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )?
    };
    if handle.is_invalid() {
        return Err(windows::core::Error::from_thread());
    }

    let mut message = MSG::default();
    loop {
        // `GetMessage` returns 0 for `WM_QUIT` and -1 on failure. Both end the
        // loop; the process is on its way out in either case.
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if status.0 == 0 || status.0 == -1 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let shutdown = SHUTDOWN.load(Ordering::Acquire);
    let show = SHOW.load(Ordering::Acquire);
    if show != 0 && msg == show && wparam.0 == MAGIC {
        if let Some(app) = APP.get() {
            if let Some(state) = app.try_state::<AppState>() {
                if !state.shared.visible() {
                    state.shared.send(Request::Toggle);
                }
            }
        }
        return LRESULT(1);
    }
    if shutdown != 0 && msg == shutdown && wparam.0 == MAGIC {
        // Leave the `SendMessage` that delivered this before the runtime
        // starts tearing the process down, or the installer waits on a
        // window procedure that is waiting on the installer.
        if let Some(app) = APP.get() {
            let app = app.clone();
            let _ = std::thread::Builder::new()
                .name("traylist-shutdown".into())
                .spawn(move || app.exit(0));
        }
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
