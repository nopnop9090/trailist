//! Replaying a click on an icon that only exists inside the native flyout.
//!
//! We cannot invoke the icon's menu directly: the shell hands those clicks to
//! the owning application's window message loop, and the message is only routed
//! while the icon is on screen. So the click is replayed as real input against
//! the icon's own coordinates, with the flyout put back underneath our overlay
//! for the two hundred milliseconds that takes.

use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::island;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// How long the flyout needs to be back on screen before its icons accept a
/// click, and how long to leave it up afterwards so the owning application can
/// handle the click before we take the window away again.
const SETTLE_BEFORE_CLICK: Duration = Duration::from_millis(120);
const SETTLE_AFTER_CLICK: Duration = Duration::from_millis(170);

/// Moves the pointer and sends a real button press.
pub fn click_at(x: i32, y: i32, button: Button) -> anyhow::Result<()> {
    warp(x, y)?;
    std::thread::sleep(Duration::from_millis(40));

    let (down, up) = match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    };
    send(down, 0, 0)?;
    std::thread::sleep(Duration::from_millis(30));
    send(up, 0, 0)
}

/// Moves the pointer to a screen position.
///
/// `SetCursorPos` is not part of the generated bindings, so the pointer is moved
/// the way the input stack itself does it: an absolute `SendInput` move in
/// virtual-desktop coordinates, which also covers secondary monitors.
fn warp(x: i32, y: i32) -> anyhow::Result<()> {
    let (left, top, width, height) = virtual_desktop();
    let nx = ((i64::from(x - left) * 65_535) / i64::from(width - 1).max(1)) as i32;
    let ny = ((i64::from(y - top) * 65_535) / i64::from(height - 1).max(1)) as i32;
    let flags = MOUSE_EVENT_FLAGS(
        MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_VIRTUALDESK.0,
    );
    send(flags, nx, ny)
}

/// The bounding box of every monitor, which is what absolute moves are relative
/// to once `MOUSEEVENTF_VIRTUALDESK` is set.
fn virtual_desktop() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

fn send(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32) -> anyhow::Result<()> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    anyhow::ensure!(sent == 1, "SendInput rejected the input");
    Ok(())
}

/// Puts the flyout back and clicks one of its icons.
///
/// Returns `false` when the flyout refused to come back, which means explorer
/// and the window are out of step and the caller should re-open the list via the
/// chevron instead of clicking into empty space.
pub fn replay(
    island_window: HWND,
    rect: island::Rect,
    x: i32,
    y: i32,
    button: Button,
) -> anyhow::Result<bool> {
    island::show_at(island_window, rect);
    std::thread::sleep(SETTLE_BEFORE_CLICK);

    if !island::is_visible(island_window) {
        return Ok(false);
    }

    let outcome = click_at(x, y, button);
    std::thread::sleep(SETTLE_AFTER_CLICK);
    island::hide(island_window);
    outcome.map(|_| true)
}