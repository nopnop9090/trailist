//! Sitting on `Shell_TrayWnd` and, once the panel is connected, on the overflow
//! flyout window.
//!
//! The subclass is installed from the taskbar's own thread. `SetWindowLongPtr`
//! from a random thread is a race against the window procedure; a
//! `WH_CALLWNDPROC` hook runs on the thread that owns the window, and that is
//! where the swap happens. The hook is removed as soon as the swap is done.
//! A window procedure that stays behind after a `FreeLibrary` is how a host
//! takes explorer down, so this DLL is never unloaded.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CallNextHookEx, CallWindowProcW, FindWindowW, GetClassNameW,
    GetPropW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    RegisterWindowMessageW,
    SendNotifyMessageW, SetPropW, SetWindowLongPtrW, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, HHOOK, WINDOWPOS, WM_COPYDATA, WM_NULL, WM_SHOWWINDOW,
    WM_WINDOWPOSCHANGING, SWP_HIDEWINDOW, SWP_SHOWWINDOW, SW_HIDE, WH_CALLWNDPROC,
};

use crate::icon;
use crate::log;
use crate::parse::{self, NotifyOp};
use crate::state::{self, Anchor, IconRecord};

const FLYOUT_CLASS: &str = "TopLevelWindowForOverflowXamlIsland";
const TRAY_CLASS: &str = "Shell_TrayWnd";
const PROP: &str = "TrayListHost";

static MODULE: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static PENDING: AtomicIsize = AtomicIsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
static TRAY_OLD: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_OLD: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_HWND: AtomicIsize = AtomicIsize::new(0);
static SPY: AtomicBool = AtomicBool::new(false);
static SPY_SEEN: AtomicU32 = AtomicU32::new(0);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn remember_module(module: isize) {
    MODULE.store(module, Ordering::SeqCst);
}

pub fn find_tray() -> Option<HWND> {
    let class = wide(TRAY_CLASS);
    let handle = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
    if handle.is_invalid() {
        None
    } else {
        Some(handle)
    }
}

pub fn subclass_tray() -> bool {
    let Some(tray) = find_tray() else {
        log::line("tray window not found");
        return false;
    };
    install(tray)
}

fn install(hwnd: HWND) -> bool {
    if marked(hwnd) {
        return true;
    }
    let mut process = 0u32;
    // The return value is the thread. The out parameter is the process, and
    // using that as a thread id makes SetWindowsHookEx fail.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    if thread == 0 {
        return false;
    }
    // Already on the window's thread: swap directly. The hook exists for the
    // worker thread, which is not that thread.
    if unsafe { GetCurrentThreadId() } == thread {
        return swap(hwnd);
    }

    // The hook runs in this process, on one of its threads. For that case the
    // module handle must be null; a DLL handle is only for injecting the hook
    // into some other process.
    INSTALLED.store(false, Ordering::SeqCst);
    PENDING.store(hwnd.0 as isize, Ordering::SeqCst);
    let hook = unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(install_hook), None, thread) };
    let Ok(hook) = hook else {
        log::line("install hook failed");
        return false;
    };
    HOOK.store(hook.0 as isize, Ordering::SeqCst);
    unsafe { SendNotifyMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0)) };
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !INSTALLED.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe { UnhookWindowsHookEx(hook) };
    HOOK.store(0, Ordering::SeqCst);
    PENDING.store(0, Ordering::SeqCst);
    let ok = marked(hwnd);
    if ok {
        log::line(&format!("subclassed 0x{:X}", hwnd.0 as isize));
    } else {
        log::line(&format!("subclass timed out for 0x{:X}", hwnd.0 as isize));
    }
    ok
}

unsafe extern "system" fn install_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let _ = std::panic::catch_unwind(|| {
            let hwnd = PENDING.load(Ordering::SeqCst);
            if hwnd != 0 {
                swap(HWND(hwnd as *mut _));
                INSTALLED.store(true, Ordering::SeqCst);
            }
            if SPY.load(Ordering::SeqCst) {
                note_spy(lparam);
            }
        });
    }
    let hook = HHOOK(HOOK.load(Ordering::SeqCst) as *mut _);
    CallNextHookEx(Some(hook), code, wparam, lparam)
}

fn swap(hwnd: HWND) -> bool {
    if marked(hwnd) {
        return true;
    }
    let class = class_of(hwnd);
    let proc = if class == FLYOUT_CLASS {
        flyout_proc as usize as isize
    } else {
        tray_proc as usize as isize
    };
    let previous = unsafe { SetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_WNDPROC, proc) };
    if previous == 0 {
        log::line(&format!("SetWindowLongPtr failed for {class}"));
        return false;
    }
    if class == FLYOUT_CLASS {
        FLYOUT_OLD.store(previous, Ordering::SeqCst);
        FLYOUT_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    } else {
        TRAY_OLD.store(previous, Ordering::SeqCst);
    }
    let name = wide(PROP);
    unsafe { SetPropW(hwnd, PCWSTR(name.as_ptr()), Some(windows::Win32::Foundation::HANDLE(1 as *mut _))) };
    true
}

fn marked(hwnd: HWND) -> bool {
    let name = wide(PROP);
    !unsafe { GetPropW(hwnd, PCWSTR(name.as_ptr())) }.is_invalid()
}

fn class_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        return std::panic::catch_unwind(|| on_copydata(hwnd, msg, wparam, lparam))
            .unwrap_or(LRESULT(0));
    }
    call_old(TRAY_OLD.load(Ordering::SeqCst), hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn flyout_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let _ = std::panic::catch_unwind(|| {
        if state::client() && !state::broken() {
            swallow_show(msg, wparam, lparam);
        }
    });
    call_old(FLYOUT_OLD.load(Ordering::SeqCst), hwnd, msg, wparam, lparam)
}

fn call_old(old: isize, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if old == 0 {
        return LRESULT(0);
    }
    unsafe {
        CallWindowProcW(
            Some(std::mem::transmute::<isize, unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(old)),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    }
}

fn swallow_show(msg: u32, wparam: WPARAM, lparam: LPARAM) {
    if msg == WM_WINDOWPOSCHANGING && lparam.0 != 0 {
        let pos = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
        if pos.flags.0 & SWP_SHOWWINDOW.0 != 0 {
            pos.flags.0 |= SWP_HIDEWINDOW.0;
            pos.flags.0 &= !SWP_SHOWWINDOW.0;
            state::request_open();
        }
        return;
    }
    if msg == WM_SHOWWINDOW && wparam.0 != 0 {
        state::request_open();
    }
}

fn on_copydata(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if lparam.0 == 0 {
        return call_old(TRAY_OLD.load(Ordering::SeqCst), hwnd, msg, wparam, lparam);
    }
    let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
    let bytes = if data.lpData.is_null() || data.cbData == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize) }
    };
    let seen = SPY_SEEN.fetch_add(1, Ordering::Relaxed);
    if seen < 2 {
        log::line(&format!(
            "copydata dwData={} cbData={}",
            data.dwData, data.cbData
        ));
        log::hex("payload", bytes);
    }

    // Explorer has to copy the icon before we read it. The read uses its own
    // `CopyIcon`, but a snapshot taken first still raced icons that replace
    // their bitmap on every update (Process Lasso) and left the flyout blank.
    let notify = if data.dwData == parse::COPYDATA_NOTIFY {
        match parse::parse(data.dwData, bytes) {
            Some(op) => Some(op),
            None => {
                log::line("notify copydata did not match the 32-bit signature layout");
                state::note_broken();
                None
            }
        }
    } else {
        None
    };

    let result = call_old(TRAY_OLD.load(Ordering::SeqCst), hwnd, msg, wparam, lparam);
    if let Some(op) = notify {
        accept(op);
    }
    if data.dwData == parse::COPYDATA_RECT {
        if let Some(query) = parse::parse_rect(data.dwData, bytes) {
            if let Some(anchor) = state::gesture_anchor(&rect_key(&query)) {
                return rect_lresult(&query, anchor, result);
            }
        }
    }
    result
}

fn rect_key(query: &parse::RectQuery) -> String {
    if let Some(guid) = query.guid {
        format!("guid:{}", parse::guid_text(&guid))
    } else {
        format!("hwnd:{}:id:{}", query.hwnd, query.id)
    }
}

/// `Shell_NotifyIconGetRect` on this build asks twice. Command 1 returns
/// `left` in the low 16 bits and `top` in the high 16. Command 2 returns
/// width and height the same way. The probe's own icon came back as
/// `(4911,1438)-(4943,1486)` from exactly those two results (`0x059E132F`
/// and `0x00300020`).
fn rect_lresult(query: &parse::RectQuery, anchor: state::Anchor, original: LRESULT) -> LRESULT {
    let width = (anchor.right - anchor.left).max(1);
    let height = (anchor.bottom - anchor.top).max(1);
    let packed = match query.command {
        1 => pack16(anchor.left, anchor.top),
        2 => pack16(width, height),
        _ => return original,
    };
    log::line(&format!(
        "rect substituted cmd {} -> 0x{packed:08X}",
        query.command
    ));
    LRESULT(packed as isize)
}

fn pack16(low: i32, high: i32) -> u32 {
    ((high as i16 as u16 as u32) << 16) | (low as i16 as u16 as u32)
}

fn accept(op: NotifyOp) {
    let png = if op.flags & parse::NIF_ICON != 0 && !op.shared_icon {
        icon::png_data_url(op.icon).unwrap_or_default()
    } else if op.flags & parse::NIF_ICON != 0 {
        // A shared icon may be freed by nobody; reading it is still just a read.
        icon::png_data_url(op.icon).unwrap_or_default()
    } else {
        String::new()
    };
    state::apply(&op, png);
}

/// While the panel is driving an icon, `Shell_NotifyIconGetRect` has to name
/// the row and not the hidden stock rectangle.
///
/// shell32 asks the taskbar for that rectangle. When the request is the
/// documented notify payload it is not a query, so this only rewrites a
/// buffer we have identified as carrying a `RECT` for the gestured icon.
/// The live probe records any other `dwData` so the rewrite can be pointed
/// at the real reply. Until that shape is known, version-4 callbacks already
/// carry the row's point in `wParam`.
pub fn note_geometry_gap() {
    // Kept as the place a recognised GetRect payload is rewritten. See
    // `on_copydata` for the capture that decides whether one exists.
    let _ = Shell_NotifyIconGetRect;
    let _ = std::mem::size_of::<NOTIFYICONIDENTIFIER>();
}

pub fn spy_for(duration: Duration) {
    let Some(tray) = find_tray() else {
        return;
    };
    let mut process = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(tray, Some(&mut process)) };
    if thread == 0 {
        return;
    }
    SPY.store(true, Ordering::SeqCst);
    let hook = unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(install_hook), None, thread) };
    let Ok(hook) = hook else {
        SPY.store(false, Ordering::SeqCst);
        return;
    };
    HOOK.store(hook.0 as isize, Ordering::SeqCst);
    std::thread::sleep(duration);
    SPY.store(false, Ordering::SeqCst);
    unsafe { UnhookWindowsHookEx(hook) };
    HOOK.store(0, Ordering::SeqCst);
    log::line("spy hook removed");
}

fn note_spy(lparam: LPARAM) {
    #[repr(C)]
    struct Cwp {
        lparam: LPARAM,
        wparam: WPARAM,
        message: u32,
        hwnd: HWND,
    }
    if lparam.0 == 0 {
        return;
    }
    let msg = unsafe { &*(lparam.0 as *const Cwp) };
    if msg.message != WM_COPYDATA || msg.lparam.0 == 0 {
        return;
    }
    let class = class_of(msg.hwnd);
    if class == TRAY_CLASS {
        return;
    }
    let data = unsafe { &*(msg.lparam.0 as *const COPYDATASTRUCT) };
    log::line(&format!(
        "spy {class} dwData={} cbData={}",
        data.dwData, data.cbData
    ));
}

/// Hide the overflow window while TrayList is connected, and subclass it so
/// the next show never paints.
pub fn watch_flyout() {
    let Some(hwnd) = find_flyout() else {
        return;
    };
    if !marked(hwnd) {
        install(hwnd);
    }
    if state::client() && !state::broken() && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        state::request_open();
    }
}

fn find_flyout() -> Option<HWND> {
    let class = wide(FLYOUT_CLASS);
    let handle = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
    if handle.is_invalid() {
        None
    } else {
        Some(handle)
    }
}

pub fn broadcast_taskbar_created() {
    let name = wide("TaskbarCreated");
    let message = unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) };
    if message == 0 {
        log::line("TaskbarCreated is not registered");
        return;
    }
    unsafe {
        SendNotifyMessageW(
            windows::Win32::UI::WindowsAndMessaging::HWND_BROADCAST,
            message,
            WPARAM(0),
            LPARAM(0),
        )
    };
    log::line("broadcast TaskbarCreated");
}

/// Posts the notification the shell would have posted for a pointer event.
pub fn notify(record: &IconRecord, event: u32, point: POINT) {
    let hwnd = HWND(record.hwnd as *mut _);
    if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool() {
        return;
    }
    // The click landed in TrayList, so the target is not the foreground process.
    // Explorer normally grants that right before it posts the icon's callback.
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != 0 {
        let _ = unsafe { AllowSetForegroundWindow(pid) };
    }
    let version = callback_version(record);
    if version >= parse::VERSION_4 {
        post_v4(hwnd, record, event, point);
    } else {
        post_classic(hwnd, record, event, point);
    }
}

fn post_v4(hwnd: HWND, record: &IconRecord, event: u32, point: POINT) {
    if record.callback == 0 {
        return;
    }
    let x = point.x as i16 as u16 as u32;
    let y = point.y as i16 as u16 as u32;
    let wparam = ((y << 16) | x) as usize;
    let lparam = ((record.id & 0xFFFF) << 16) | (event & 0xFFFF);
    unsafe {
        PostMessageW(
            Some(hwnd),
            record.callback,
            WPARAM(wparam),
            LPARAM(lparam as isize),
        )
    };
}

fn post_classic(hwnd: HWND, record: &IconRecord, event: u32, _point: POINT) {
    if record.callback == 0 {
        return;
    }
    unsafe {
        PostMessageW(
            Some(hwnd),
            record.callback,
            WPARAM(record.id as usize),
            LPARAM(event as isize),
        )
    };
}

pub fn anchor_point(anchor: Anchor) -> POINT {
    POINT {
        x: anchor.left + 18,
        y: anchor.top + (anchor.bottom - anchor.top) / 2,
    }
}

pub fn hold_gesture(key: &str, anchor: Anchor) {
    state::begin_gesture(key, anchor, Duration::from_secs(3));
}

/// Rewrites a `RECT` inside a geometry reply when one is sitting in `bytes`
/// and a gesture is active for `key`. Returns whether it changed anything.
pub fn rewrite_rect(bytes: &mut [u8], anchor: Anchor) -> bool {
    let target = RECT {
        left: anchor.left,
        top: anchor.top,
        right: anchor.right.max(anchor.left + 16),
        bottom: anchor.bottom.max(anchor.top + 16),
    };
    // A geometry reply is small. Walk every 4-byte-aligned candidate that
    // already looks like a screen rectangle and replace the first one.
    if bytes.len() < 16 {
        return false;
    }
    let mut offset = 0;
    while offset + 16 <= bytes.len() {
        let left = i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let top = i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        let right = i32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap());
        let bottom = i32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        let plausible = right > left && bottom > top && right - left < 512 && bottom - top < 512
            && left.abs() < 20_000 && top.abs() < 20_000;
        if plausible {
            bytes[offset..offset + 4].copy_from_slice(&target.left.to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&target.top.to_le_bytes());
            bytes[offset + 8..offset + 12].copy_from_slice(&target.right.to_le_bytes());
            bytes[offset + 12..offset + 16].copy_from_slice(&target.bottom.to_le_bytes());
            return true;
        }
        offset += 4;
    }
    false
}

pub fn flyout_visible() -> bool {
    find_flyout()
        .map(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .unwrap_or(false)
}

pub fn taskbar_anchor() -> Option<RECT> {
    let tray = find_tray()?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(tray, &mut rect) }.ok()?;
    Some(rect)
}

/// Message ids the shell posts for pointer gestures.
pub mod events {
    pub const NIN_SELECT: u32 = 0x0400;
    pub const NIN_POPUPOPEN: u32 = 0x0406;
    pub const NIN_POPUPCLOSE: u32 = 0x0407;
    pub const WM_CONTEXTMENU: u32 = 0x007B;
    pub const WM_LBUTTONDOWN: u32 = 0x0201;
    pub const WM_LBUTTONUP: u32 = 0x0202;
    pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
    pub const WM_RBUTTONUP: u32 = 0x0205;
    pub const WM_MOUSEMOVE: u32 = 0x0200;
}

/// Version the shell would use. Only `NIM_SETVERSION` changes it. Until then
/// the icon is version 0, and a version-4 `WM_CONTEXTMENU` is ignored.
fn callback_version(record: &IconRecord) -> u32 {
    if record.version_known {
        record.version
    } else {
        0
    }
}

pub fn deliver(record: &IconRecord, kind: &str, anchor: Anchor) {
    let point = anchor_point(anchor);
    let version = callback_version(record);
    hold_gesture(&record.key, anchor);
    // The double-click alone. A button-up or `NIN_SELECT` beside it is the
    // single-click action (Steam opens its menu from that).
    if kind == "double" {
        notify(record, events::WM_LBUTTONDBLCLK, point);
        return;
    }
    if version >= parse::VERSION_4 {
        let event = match kind {
            "enter" => events::NIN_POPUPOPEN,
            "leave" => events::NIN_POPUPCLOSE,
            "left" => events::NIN_SELECT,
            "right" => events::WM_CONTEXTMENU,
            _ => return,
        };
        notify(record, event, point);
    } else {
        match kind {
            "enter" => notify(record, events::WM_MOUSEMOVE, point),
            "left" => {
                notify(record, events::WM_LBUTTONDOWN, point);
                notify(record, events::WM_LBUTTONUP, point);
            }
            "right" => {
                // The mouse button, which is what a version-0 window shows its
                // menu from. `WM_CONTEXTMENU` is the keyboard form; sending both
                // opens the menu twice.
                notify(record, events::WM_RBUTTONUP, point);
            }
            _ => {}
        }
    }
    if kind == "leave" {
        state::end_gesture();
    }
}
