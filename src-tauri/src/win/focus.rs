//! Raising our own window to the foreground.
//!
//! The panel appears because the user clicked the *taskbar*, so our process is
//! not the foreground process and Windows refuses a plain `SetForegroundWindow`.
//! Joining the foreground thread's input queue first is the standard way around
//! that, and it is what makes the filter box and the keyboard usable.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn is_foreground(handle: HWND) -> bool {
    window_address(foreground()) == window_address(handle)
}

/// Tries to make `handle` the foreground window, politely but persistently.
pub fn raise(handle: HWND) {
    if handle.is_invalid() || is_foreground(handle) {
        return;
    }

    unsafe {
        // The return value is the thread that owns the window, which is exactly
        // what has to be joined.
        let foreground_thread = GetWindowThreadProcessId(foreground(), None);
        let target_thread = GetWindowThreadProcessId(handle, None);
        let own_thread = GetCurrentThreadId();

        // Attaching to both queues makes the call look like it comes from the
        // thread that currently owns the foreground, which is the only thing
        // Windows actually checks here.
        let attached_foreground =
            foreground_thread != own_thread && AttachThreadInput(own_thread, foreground_thread, true).as_bool();
        let attached_target =
            target_thread != own_thread && AttachThreadInput(own_thread, target_thread, true).as_bool();

        let _ = SetForegroundWindow(handle);

        if attached_target {
            let _ = AttachThreadInput(own_thread, target_thread, false);
        }
        if attached_foreground {
            let _ = AttachThreadInput(own_thread, foreground_thread, false);
        }
    }
}

/// Compare two window handles by value, without pretending they are pointers.
fn window_address(handle: HWND) -> isize {
    handle.0 as isize
}