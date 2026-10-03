//! Locating and steering the Windows 11 tray overflow flyout window.
//!
//! The classic Windows 10 route into the tray (a `ToolbarWindow32` inside
//! `TrayNotifyWnd`) is gone: on Windows 11 the notification area and its
//! overflow flyout are XAML islands. The flyout is still a real window though,
//! and that is the handle everything else hangs off.

use std::ffi::c_void;
use std::sync::Mutex;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowRect, IsWindowVisible, SetWindowPos, ShowWindow, SW_HIDE,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNOACTIVATE,
};

/// The flyout that holds the hidden tray icons.
pub const ISLAND_CLASS: &str = "TopLevelWindowForOverflowXamlIsland";
/// The main taskbar window, which hosts the chevron we can click.
pub const TASKBAR_CLASS: &str = "Shell_TrayWnd";
/// A taskbar on any monitor that is not the primary one.
pub const SECONDARY_TASKBAR_CLASS: &str = "Shell_SecondaryTrayWnd";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Wrap a raw handle value, as handed to the frontend or over IPC.
pub fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// The raw value of a handle, which is what we pass across threads.
pub fn raw(handle: HWND) -> isize {
    handle.0 as isize
}

fn class_of(handle: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(handle, &mut buffer) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

/// Every top-level window of the given class.
pub fn all_by_class(class: &str) -> Vec<HWND> {
    struct Search<'a> {
        class: &'a str,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn walk(handle: HWND, arg: LPARAM) -> BOOL {
        let search = &mut *(arg.0 as *mut Search);
        if class_of(handle) == search.class {
            search.found.push(handle);
        }
        BOOL(1)
    }

    let mut search = Search { class, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(walk), LPARAM(&mut search as *mut _ as isize));
    }
    search.found
}

/// The first window of the given class, if any.
pub fn find_by_class(class: &str) -> Option<HWND> {
    all_by_class(class).into_iter().next()
}

/// Every overflow flyout window on the desktop, hidden or not.
pub fn all() -> Vec<HWND> {
    all_by_class(ISLAND_CLASS)
}

/// The flyout windows that are currently on screen.
pub fn visible() -> Vec<HWND> {
    all().into_iter().filter(|handle| is_visible(*handle)).collect()
}

pub fn is_visible(handle: HWND) -> bool {
    unsafe { IsWindowVisible(handle).as_bool() }
}

pub fn rect(handle: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(handle, &mut rect).ok()? };
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }
    Some(Rect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    })
}

/// The work area of the monitor a point falls on, which is what the panel must
/// stay inside: it already has the taskbar subtracted.
pub fn work_area_for_point(x: i32, y: i32) -> Option<Rect> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    Some(Rect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    })
}

/// Finds a window by its title.
///
/// Used for our own panel, which we can name but whose handle Tauri hands out in
/// a `windows` version we do not share. `FindWindowW` is tried first and a
/// top-level sweep is the fallback, because a title match on a window that is
/// currently hidden is exactly the case that has to keep working.
pub fn find_by_title(title: &str) -> Option<HWND> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowTextW};

    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    if let Ok(handle) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) } {
        if !handle.is_invalid() {
            return Some(handle);
        }
    }

    struct Search<'a> {
        title: &'a str,
        found: Option<HWND>,
    }

    unsafe extern "system" fn walk(handle: HWND, arg: LPARAM) -> BOOL {
        let search = &mut *(arg.0 as *mut Search);
        let mut buffer = [0u16; 256];
        let length = GetWindowTextW(handle, &mut buffer);
        if length > 0 && String::from_utf16_lossy(&buffer[..length as usize]) == search.title {
            search.found = Some(handle);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut search = Search { title, found: None };
    unsafe {
        let _ = EnumWindows(Some(walk), LPARAM(&mut search as *mut _ as isize));
    }
    search.found
}

/// The monitor's scaling factor, taken from a window so it follows the monitor
/// that window is actually on rather than the primary one.
pub fn dpi_scale(handle: HWND) -> f64 {
    use windows::Win32::UI::HiDpi::GetDpiForWindow;

    let reference = if handle.is_invalid() {
        find_by_class(TASKBAR_CLASS)
    } else {
        Some(handle)
    };
    match reference {
        Some(window) => {
            let dpi = unsafe { GetDpiForWindow(window) };
            if dpi == 0 {
                1.0
            } else {
                f64::from(dpi) / 96.0
            }
        }
        None => 1.0,
    }
}

/// Whether a window belongs to the shell's own tray surface: the taskbar, a
/// secondary taskbar, or the overflow flyout.
///
/// Used to tell "the user clicked the chevron" apart from "the user clicked
/// somewhere else", which are the same event as far as focus is concerned.
pub fn is_shell_surface(handle: HWND) -> bool {
    if handle.is_invalid() {
        return false;
    }
    let class = class_of(handle);
    class == TASKBAR_CLASS || class == ISLAND_CLASS || class == SECONDARY_TASKBAR_CLASS
}

/// The last place the flyout was seen on screen.
///
/// The flyout gets parked at `PARK_X` while it is read, and if explorer ever
/// shows it again without repositioning it, the place it is found at is useless
/// both for placing our panel and for putting the flyout back afterwards. Keeping
/// the last good position makes that case harmless.
static REMEMBERED: Mutex<Option<Rect>> = Mutex::new(None);

/// The last on-screen position of the flyout, if one has been seen.
pub fn remembered_rect() -> Option<Rect> {
    *REMEMBERED.lock().unwrap()
}

/// Records where the flyout really sits, so a later reading that finds it parked
/// can still work out where it belongs.
pub fn remember_rect(rect: Rect) {
    *REMEMBERED.lock().unwrap() = Some(rect);
}

/// Whether a rectangle is somewhere a user could actually see it.
pub fn is_on_screen(rect: Rect) -> bool {
    rect.left > -10_000 && rect.top > -10_000 && rect.width() > 0 && rect.height() > 0
}

/// Off-screen resting place for the flyout.
///
/// Far enough left that no monitor can be there, but still *shown*. That
/// distinction is the whole trick: a hidden window stops answering UI Automation
/// and renders as a blank bitmap, while one that is merely off screen keeps both
/// its content and its answers, so the icon grid can be read and captured while
/// the user sees nothing.
pub const PARK_X: i32 = -32_000;

/// Moves the flyout out of sight without hiding it.
pub fn park(handle: HWND, rect: Rect) -> bool {
    unsafe {
        SetWindowPos(
            handle,
            None,
            PARK_X,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .is_ok()
    }
}

/// Moves the flyout back without showing it, so a hidden flyout is left where the
/// shell expects to find it.
pub fn move_to(handle: HWND, rect: Rect) {
    unsafe {
        let _ = SetWindowPos(
            handle,
            None,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Puts the flyout back where the shell had it and brings it up without taking
/// the keyboard, ready for a replayed click.
pub fn show_at(handle: HWND, rect: Rect) {
    unsafe {
        let _ = SetWindowPos(
            handle,
            None,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(handle, SW_SHOWNOACTIVATE);
    }
}

/// Takes the flyout off screen without telling explorer, which leaves its own
/// state alone: the next chevron click opens it again exactly as before.
pub fn hide(handle: HWND) {
    unsafe {
        let _ = ShowWindow(handle, SW_HIDE);
    }
}

/// Brings the flyout back for a single click without activating it, so the click
/// lands on the icon instead of being swallowed by a focus change.
pub fn show_without_activating(handle: HWND) {
    unsafe {
        let _ = ShowWindow(handle, SW_SHOWNOACTIVATE);
    }
}