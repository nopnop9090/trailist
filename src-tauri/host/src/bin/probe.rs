//! Loads the host into explorer and sends one known icon through
//! `Shell_NotifyIcon`, then prints the host log.
//!
//! This is the check that the documented `WM_COPYDATA` layout still arrives
//! on this build. It does not open the panel and it removes the icon before
//! it exits.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// `WM_APP`. The probe's tray callback, so a delivered gesture is observable
/// without destroying the window.
const CALLBACK: u32 = 0x8000;
static LAST_EVENT: AtomicU32 = AtomicU32::new(0);

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleFileNameExW};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_TIP,
    NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
    NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetWindowThreadProcessId,
    LoadIconW, RegisterClassW, RegisterWindowMessageW, SendMessageW, HICON, IDI_APPLICATION,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW, WM_DESTROY, WS_OVERLAPPED,
};

fn main() {
    let dll = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("trailist_host.dll")))
        .expect("probe sits next to trailist_host.dll");
    if !dll.exists() {
        eprintln!("missing {}", dll.display());
        std::process::exit(1);
    }

    let log = std::env::temp_dir().join("trailist-host.log");
    let _ = std::fs::write(&log, "");
    let spy = std::env::temp_dir().join("trailist-host-spy");
    let _ = std::fs::write(&spy, b"1");

    match inject(&dll) {
        Ok(()) => println!("injected {}", dll.display()),
        Err(error) => {
            eprintln!("inject failed: {error}");
            let _ = std::fs::remove_file(&spy);
            std::process::exit(1);
        }
    }

    if !wait_for_log(&log, "subclassed", Duration::from_secs(5)) {
        eprintln!("the host did not subclass the tray");
        dump_log(&log);
        let _ = std::fs::remove_file(&spy);
        std::process::exit(1);
    }

    let window = message_window();
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_GUID,
        uCallbackMessage: CALLBACK,
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        guidItem: windows::core::GUID::from_u128(0x4e7a_0c31_6b2d_4f18_9a55_c0de_0000_0001),
        ..Default::default()
    };
    write_tip(&mut data, "TrayListProbeIcon");
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    println!("NIM_ADD -> {added:?}");
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let versioned = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
    println!("NIM_SETVERSION -> {versioned:?}");

    let id = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: window,
        uID: 1,
        guidItem: data.guidItem,
    };
    match unsafe { Shell_NotifyIconGetRect(&id) } {
        Ok(rect) => println!(
            "GetRect stock -> ({},{}-{},{})",
            rect.left, rect.top, rect.right, rect.bottom
        ),
        Err(error) => println!("GetRect stock -> {error}"),
    }
    let pipe = match open_pipe_retry(Duration::from_secs(3)) {
        Ok(pipe) => Some(pipe),
        Err(error) => {
            println!("pipe -> {error}");
            None
        }
    };
    if let Some(pipe) = pipe {
        match pipe_call(pipe, r#"{"op":"poll","full":true}"#) {
            Ok(reply) => println!("mirror -> {}", summarise_mirror(&reply)),
            Err(error) => println!("mirror -> {error}"),
        }
        let hover = r#"{"op":"hover","key":"guid:{4E7A0C31-6B2D-4F18-9A55-C0DE00000001}","enter":true,"anchor":{"left":120,"top":240,"right":160,"bottom":272}}"#;
        match pipe_call(pipe, hover) {
            Ok(_) => println!("hover posted"),
            Err(error) => println!("hover -> {error}"),
        }
        match unsafe { Shell_NotifyIconGetRect(&id) } {
            Ok(rect) => println!(
                "GetRect gesture -> ({},{}-{},{})",
                rect.left, rect.top, rect.right, rect.bottom
            ),
            Err(error) => println!("GetRect gesture -> {error}"),
        }
        pump(Duration::from_millis(500));
        println!("callback event 0x{:X}", LAST_EVENT.load(Ordering::SeqCst));
        unsafe { windows::Win32::Foundation::CloseHandle(pipe) }.ok();
    }
    marker();
    std::thread::sleep(Duration::from_millis(400));
    let removed = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    println!("NIM_DELETE -> {removed:?}");
    std::thread::sleep(Duration::from_millis(400));
    unsafe { DestroyWindow(window) };
    let _ = std::fs::remove_file(&spy);
    dump_log(&log);
}

fn summarise_mirror(reply: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(reply) else {
        return reply.chars().take(180).collect();
    };
    let icons = value.get("icons").and_then(|icons| icons.as_array());
    let Some(icons) = icons else {
        return format!("no icons in {}", &reply.chars().take(120).collect::<String>());
    };
    let mut lines = Vec::new();
    lines.push(format!(
        "bound={} count={}",
        value.get("bound").and_then(|v| v.as_bool()).unwrap_or(false),
        icons.len()
    ));
    for icon in icons {
        let tip = icon.get("tip").and_then(|v| v.as_str()).unwrap_or("");
        let first = tip.lines().next().unwrap_or("");
        let key = icon.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let callback = icon.get("callback").and_then(|v| v.as_u64()).unwrap_or(0);
        let icon_len = icon.get("icon").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
        if first.contains("TrayListProbe") || lines.len() < 8 {
            lines.push(format!("{key} cb={callback} icon={icon_len} {first}"));
        }
    }
    lines.join(" | ")
}

fn open_pipe_retry(budget: Duration) -> Result<windows::Win32::Foundation::HANDLE, String> {
    let deadline = std::time::Instant::now() + budget;
    let mut last = String::from("pipe missing");
    while std::time::Instant::now() < deadline {
        match open_pipe() {
            Ok(pipe) => return Ok(pipe),
            Err(error) => last = error,
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(last)
}

fn open_pipe() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Pipes::{SetNamedPipeHandleState, PIPE_READMODE_MESSAGE};
    let name: Vec<u16> = "\\\\.\\pipe\\TrayList.Host"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let pipe = unsafe {
        windows::Win32::Storage::FileSystem::CreateFileW(
            windows::core::PCWSTR(name.as_ptr()),
            0x8000_0000 | 0x4000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|error| error.to_string())?;
    let mode = PIPE_READMODE_MESSAGE;
    unsafe { SetNamedPipeHandleState(pipe, Some(&mode), None, None) }.map_err(|error| error.to_string())?;
    Ok(pipe)
}

fn pipe_call(pipe: windows::Win32::Foundation::HANDLE, body: &str) -> Result<String, String> {
    use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    let mut written = 0u32;
    unsafe { WriteFile(pipe, Some(body.as_bytes()), Some(&mut written), None) }
        .map_err(|error| error.to_string())?;
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut read = 0u32;
    unsafe { ReadFile(pipe, Some(&mut buffer), Some(&mut read), None) }.map_err(|error| error.to_string())?;
    String::from_utf8(buffer[..read as usize].to_vec()).map_err(|error| error.to_string())
}

fn pump(budget: Duration) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    let deadline = std::time::Instant::now() + budget;
    let mut message = MSG::default();
    while std::time::Instant::now() < deadline {
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn write_tip(data: &mut NOTIFYICONDATAW, tip: &str) {
    for (index, unit) in tip.encode_utf16().take(127).enumerate() {
        data.szTip[index] = unit;
    }
}

fn marker() {
    let message = unsafe { RegisterWindowMessageW(w!("TrayListHost.Probe")) };
    let class: Vec<u16> = "Shell_TrayWnd".encode_utf16().chain(std::iter::once(0)).collect();
    if let Ok(tray) = unsafe { FindWindowW(windows::core::PCWSTR(class.as_ptr()), windows::core::PCWSTR::null()) } {
        unsafe { SendMessageW(tray, message, Some(WPARAM(0)), Some(LPARAM(0))) };
    }
}

fn wait_for_log(path: &std::path::Path, needle: &str, budget: Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(path)
            .unwrap_or_default()
            .contains(needle)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn dump_log(path: &std::path::Path) {
    println!("--- {}", path.display());
    println!("{}", std::fs::read_to_string(path).unwrap_or_default());
}

fn message_window() -> HWND {
    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == CALLBACK {
            LAST_EVENT.store((lp.0 as u32) & 0xFFFF, Ordering::SeqCst);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
    let class = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: unsafe { GetModuleHandleW(None).unwrap_or_default().into() },
        lpszClassName: w!("TrayListHostProbe"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("TrayListHostProbe"),
            w!("TrayListHostProbe"),
            WINDOW_STYLE(WS_OVERLAPPED.0),
            0,
            0,
            0,
            0,
            None,
            None,
            GetModuleHandleW(None).ok().map(|handle| handle.into()),
            None,
        )
        .expect("probe window")
    }
}

/// Drops a host DLL left behind by an earlier probe that never subclassed.
/// A DLL that has already swapped a window procedure must not be freed; this
/// only runs from the probe, and only when the module is still mapped.
fn unload_previous(process: windows::Win32::Foundation::HANDLE) {
    let mut modules = [windows::Win32::Foundation::HMODULE::default(); 512];
    let mut needed = 0u32;
    let listed = unsafe {
        EnumProcessModules(
            process,
            modules.as_mut_ptr(),
            std::mem::size_of_val(&modules) as u32,
            &mut needed,
        )
    };
    if listed.is_err() {
        return;
    }
    let count = (needed as usize / std::mem::size_of::<windows::Win32::Foundation::HMODULE>()).min(modules.len());
    let kernel = unsafe { GetModuleHandleW(w!("kernel32.dll")) }.ok();
    let Some(kernel) = kernel else {
        return;
    };
    let Some(free) = (unsafe { GetProcAddress(kernel, windows::core::s!("FreeLibrary")) }) else {
        return;
    };
    for module in modules.iter().take(count) {
        let mut name = [0u16; 260];
        let length = unsafe { GetModuleFileNameExW(Some(process), Some(*module), &mut name) };
        if length == 0 {
            continue;
        }
        let text = String::from_utf16_lossy(&name[..length as usize]);
        if !text.to_ascii_lowercase().ends_with("trailist_host.dll") {
            continue;
        }
        println!("unloading previous host");
        if let Ok(thread) = unsafe {
            CreateRemoteThread(
                process,
                None,
                0,
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
                >(free)),
                Some(module.0 as *const _),
                0,
                None,
            )
        } {
            unsafe { WaitForSingleObject(thread, 5000) };
        }
    }
}

fn inject(dll: &std::path::Path) -> Result<(), String> {
    let tray_class: Vec<u16> = "Shell_TrayWnd"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let tray = unsafe {
        FindWindowW(
            windows::core::PCWSTR(tray_class.as_ptr()),
            windows::core::PCWSTR::null(),
        )
    }
    .map_err(|error| error.to_string())?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(tray, Some(&mut pid)) };
    if pid == 0 {
        return Err("explorer pid missing".into());
    }
    let access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_READ
        | PROCESS_VM_WRITE;
    let process = unsafe { OpenProcess(access, false, pid) }.map_err(|error| error.to_string())?;
    unload_previous(process);
    let wide: Vec<u16> = dll
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let bytes = std::mem::size_of_val(wide.as_slice());
    let remote = unsafe {
        VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return Err("VirtualAllocEx failed".into());
    }
    let wrote = unsafe {
        WriteProcessMemory(
            process,
            remote,
            wide.as_ptr().cast(),
            bytes,
            None,
        )
    };
    if wrote.is_err() {
        unsafe { windows::Win32::System::Memory::VirtualFreeEx(process, remote, 0, MEM_RELEASE) };
        return Err("WriteProcessMemory failed".into());
    }
    let kernel = unsafe { GetModuleHandleW(w!("kernel32.dll")) }.map_err(|error| error.to_string())?;
    let load = unsafe { GetProcAddress(kernel, windows::core::s!("LoadLibraryW")) }
        .ok_or("LoadLibraryW missing")?;
    let thread = unsafe {
        CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
            >(load)),
            Some(remote),
            0,
            None,
        )
    }
    .map_err(|error| error.to_string())?;
    unsafe { WaitForSingleObject(thread, 5000) };
    let mut code = 0u32;
    unsafe { windows::Win32::System::Threading::GetExitCodeThread(thread, &mut code) };
    if code == 0 || code >= 0xC000_0000 {
        return Err(format!("LoadLibraryW failed inside explorer: 0x{code:X}"));
    }
    println!("LoadLibraryW -> 0x{code:X}");
    Ok(())
}

#[allow(dead_code)]
fn _icon_ty(icon: HICON) -> HICON {
    icon
}
