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

/// Every taskbar, primary first. A point on one of these is a click on the tray.
pub fn taskbars() -> Vec<Rect> {
    let mut bars = Vec::new();
    for class in [TASKBAR_CLASS, SECONDARY_TASKBAR_CLASS] {
        for handle in all_by_class(class) {
            if let Some(rect) = rect(handle) {
                bars.push(rect);
            }
        }
    }
    bars
}

/// The anchor touches a taskbar, so it can place the panel. A rectangle at the
/// origin does not: that is where a window is born, not where the chevron is.
pub fn beside_taskbar(anchor: Rect) -> bool {
    if anchor.width() <= 0 || anchor.height() <= 0 {
        return false;
    }
    let centre_x = anchor.left + anchor.width() / 2;
    let centre_y = anchor.top + anchor.height() / 2;
    if !point_on_monitor(centre_x, centre_y) {
        return false;
    }
    taskbars().iter().any(|bar| touches(*bar, anchor, 120))
}

/// Where the panel hangs from. The click is used when it is still on a taskbar,
/// otherwise the chevron, otherwise the tray end of the taskbar. Never the
/// origin: a missing measurement is what put the list in the top-left corner.
pub fn tray_anchor(chevron: Option<Rect>) -> Rect {
    let bars = taskbars();
    let cursor = cursor_pos();
    let Some(bar) = pick_bar(&bars, cursor, chevron) else {
        return work_corner(cursor);
    };
    let along = along_bar(bar, cursor, chevron);
    let mid_x = bar.left + bar.width() / 2;
    let mid_y = bar.top + bar.height() / 2;
    let work = work_area_for_point(mid_x, mid_y).unwrap_or(bar);
    inner_marker(bar, work, along)
}

/// Puts the window exactly where it was measured, on this thread.
///
/// The webview posts its own move, and that post can lose to the window being
/// shown at the origin it was created at. A synchronous move afterwards is the
/// one that sticks.
pub fn pin(handle: HWND, x: i32, y: i32, width: i32, height: i32) {
    if handle.is_invalid() || width <= 0 || height <= 0 {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            handle,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).ok()? };
    Some((point.x, point.y))
}

fn point_on_monitor(x: i32, y: i32) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};

    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) };
    !monitor.is_invalid()
}

/// Bottom-right of the work area under the pointer. Used only when no taskbar
/// window can be found at all.
fn work_corner(cursor: Option<(i32, i32)>) -> Rect {
    let (x, y) = cursor.unwrap_or((0, 0));
    let work = work_area_for_point(x, y).unwrap_or(Rect {
        left: 0,
        top: 0,
        right: 800,
        bottom: 600,
    });
    Rect {
        left: work.right - 1,
        right: work.right,
        top: work.bottom - 1,
        bottom: work.bottom,
    }
}

fn pick_bar(bars: &[Rect], cursor: Option<(i32, i32)>, chevron: Option<Rect>) -> Option<Rect> {
    if let Some((x, y)) = cursor {
        if let Some(bar) = bars.iter().copied().find(|bar| contains(*bar, x, y, 24)) {
            return Some(bar);
        }
    }
    if let Some(chevron) = chevron {
        if let Some(bar) = bars.iter().copied().find(|bar| touches(*bar, chevron, 8)) {
            return Some(bar);
        }
    }
    bars.first().copied()
}

fn along_bar(bar: Rect, cursor: Option<(i32, i32)>, chevron: Option<Rect>) -> i32 {
    let horizontal = bar.width() >= bar.height();
    if let Some((x, y)) = cursor {
        if contains(bar, x, y, 48) {
            return if horizontal { x } else { y };
        }
    }
    if let Some(chevron) = chevron {
        if touches(bar, chevron, 32) {
            return if horizontal {
                chevron.left + chevron.width() / 2
            } else {
                chevron.top + chevron.height() / 2
            };
        }
    }
    tray_end(bar)
}

/// The notification area is at the far end of the bar. Horizontal bars keep it
/// at the right, vertical ones at the bottom.
fn tray_end(bar: Rect) -> i32 {
    if bar.width() >= bar.height() {
        (bar.right - 64).clamp(bar.left, (bar.right - 1).max(bar.left))
    } else {
        (bar.bottom - 64).clamp(bar.top, (bar.bottom - 1).max(bar.top))
    }
}

/// One pixel on the side of the taskbar that faces the desktop.
pub(crate) fn inner_marker(bar: Rect, work: Rect, along: i32) -> Rect {
    if bar.width() >= bar.height() {
        let x = along.clamp(bar.left, (bar.right - 1).max(bar.left));
        if (bar.top - work.bottom).abs() <= (bar.bottom - work.top).abs() {
            Rect {
                left: x,
                right: x + 1,
                top: bar.top - 1,
                bottom: bar.top,
            }
        } else {
            Rect {
                left: x,
                right: x + 1,
                top: bar.bottom,
                bottom: bar.bottom + 1,
            }
        }
    } else {
        let y = along.clamp(bar.top, (bar.bottom - 1).max(bar.top));
        if (bar.left - work.right).abs() <= (bar.right - work.left).abs() {
            Rect {
                left: bar.left - 1,
                right: bar.left,
                top: y,
                bottom: y + 1,
            }
        } else {
            Rect {
                left: bar.right,
                right: bar.right + 1,
                top: y,
                bottom: y + 1,
            }
        }
    }
}

pub(crate) fn touches(bar: Rect, anchor: Rect, slack: i32) -> bool {
    let grown = Rect {
        left: bar.left - slack,
        top: bar.top - slack,
        right: bar.right + slack,
        bottom: bar.bottom + slack,
    };
    grown.left < anchor.right
        && anchor.left < grown.right
        && grown.top < anchor.bottom
        && anchor.top < grown.bottom
}

fn contains(bar: Rect, x: i32, y: i32, slack: i32) -> bool {
    x >= bar.left - slack && x < bar.right + slack && y >= bar.top - slack && y < bar.bottom + slack
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

#[cfg(test)]
mod tests {
    use super::{inner_marker, touches, Rect};

    fn bottom_bar() -> Rect {
        Rect {
            left: 0,
            top: 1392,
            right: 5120,
            bottom: 1440,
        }
    }

    #[test]
    fn the_origin_is_not_the_chevron() {
        let origin = Rect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        assert!(!touches(bottom_bar(), origin, 120));
    }

    #[test]
    fn a_flyout_sitting_on_the_taskbar_counts() {
        let flyout = Rect {
            left: 4750,
            top: 998,
            right: 4984,
            bottom: 1392,
        };
        assert!(touches(bottom_bar(), flyout, 120));
    }

    #[test]
    fn the_marker_sits_on_the_inner_edge_at_the_chevron() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 5120,
            bottom: 1392,
        };
        let marker = inner_marker(bottom_bar(), work, 4867);
        assert_eq!(marker.left, 4867);
        assert_eq!(marker.bottom, 1392);
        assert!(marker.top < marker.bottom);
    }
}