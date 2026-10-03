//! The in-explorer host: load it, then talk to it over the named pipe.
//!
//! The DLL subclasses the tray inside `explorer.exe`. This process never does.
//! If the pipe is not there, the watcher keeps the UI Automation path.

use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, POINT};
use windows::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Pipes::{SetNamedPipeHandleState, PIPE_READMODE_MESSAGE};
use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleFileNameExW};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

use crate::win::island;

const PIPE_NAME: &str = "\\\\.\\pipe\\TrayList.Host";

/// A row rectangle in screen pixels, which is what the host posts to the app.
#[derive(Debug, Clone, Copy)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// One icon from the host's mirror.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostIcon {
    pub key: String,
    #[serde(default)]
    pub hwnd: isize,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub callback: u32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub version_known: bool,
    pub tip: String,
    pub hidden: bool,
    pub icon: String,
}

impl HostIcon {
    /// The registration's window is gone. Explorer drops the icon then; the
    /// mirror only hears about it when the app sent `NIM_DELETE`, which a
    /// killed process usually does not.
    pub fn window_alive(&self) -> bool {
        if self.hwnd == 0 {
            return false;
        }
        let window = windows::Win32::Foundation::HWND(self.hwnd as *mut std::ffi::c_void);
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)).as_bool() }
    }

    /// Classic click, for an icon that never called `NIM_SETVERSION`.
    ///
    /// The shell leaves that icon at version 0: `wParam` is the id and `lParam`
    /// is the mouse message. A version-4 `WM_CONTEXTMENU` does not match, so
    /// the window returns without opening anything. The loaded host still
    /// guesses version 4 for those icons; this sends the message the window
    /// actually handles. A host that reports `unknownIsClassic` already does.
    pub fn post_legacy(&self, right: bool) {
        if self.callback == 0 || !self.window_alive() {
            return;
        }
        let window = hwnd_from(self.hwnd);
        // Right-click is the button-up the shell sends for version 0. Sending
        // `WM_CONTEXTMENU` as well opens a second menu in windows that handle
        // both. Left-click is the down/up pair.
        let messages: &[u32] = if right { &[0x0205] } else { &[0x0201, 0x0202] };
        for message in messages {
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::SendNotifyMessageW(
                    window,
                    self.callback,
                    windows::Win32::Foundation::WPARAM(self.id as usize),
                    windows::Win32::Foundation::LPARAM(*message as isize),
                )
            };
        }
    }

    /// Classic hover. Version 0 windows are told with `WM_MOUSEMOVE`.
    pub fn post_legacy_move(&self) {
        if self.callback == 0 || !self.window_alive() {
            return;
        }
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(hwnd_from(self.hwnd)),
                self.callback,
                windows::Win32::Foundation::WPARAM(self.id as usize),
                windows::Win32::Foundation::LPARAM(0x0200),
            )
        };
    }
}

/// Lets `hwnd`'s process call `SetForegroundWindow` for the menu it is about
/// to open. The click landed here, so this process is allowed to grant that;
/// Explorer, which did not receive the click, often is not.
pub fn grant_foreground(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd_from(hwnd), Some(&mut pid)) };
    if pid != 0 {
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
    }
}

/// Executable that owns `hwnd`, so a row with no tooltip can still be matched
/// to the shell's `NotifyIconSettings` entry.
pub fn owner_exe(hwnd: isize) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let window = windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void);
    let mut pid = 0u32;
    if unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) } == 0 || pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0u16; 1024];
    let mut length = buffer.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    read.ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

/// The name Windows shows for an executable when the tray tooltip is empty.
///
/// `FileDescription` is what Explorer uses ("NVIDIA Broadcast"). The file name
/// is the fallback when the binary has no version resource.
pub fn program_name(path: &str) -> Option<String> {
    version_string(path, "FileDescription")
        .or_else(|| version_string(path, "ProductName"))
        .or_else(|| {
            std::path::Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|stem| !stem.is_empty())
                .map(str::to_string)
        })
}

fn version_string(path: &str, key: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::core::PCWSTR;
    if path.is_empty() {
        return None;
    }
    let file = wide(path);
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(file.as_ptr()), None, size, block.as_mut_ptr().cast()) }
        .ok()?;
    let (lang, codepage) = file_translation(&block).unwrap_or((0x0409, 0x04B0));
    let query = format!("\\StringFileInfo\\{lang:04X}{codepage:04X}\\{key}");
    let query = wide(&query);
    let mut ptr = std::ptr::null_mut();
    let mut len = 0u32;
    let ok = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(query.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    if !ok.as_bool() || ptr.is_null() || len == 0 {
        return None;
    }
    let words = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize) };
    let end = words.iter().position(|unit| *unit == 0).unwrap_or(words.len());
    let text = String::from_utf16_lossy(&words[..end]).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn file_translation(block: &[u8]) -> Option<(u16, u16)> {
    use windows::Win32::Storage::FileSystem::VerQueryValueW;
    use windows::core::PCWSTR;
    let query = wide("\\VarFileInfo\\Translation");
    let mut ptr = std::ptr::null_mut();
    let mut len = 0u32;
    let ok = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(query.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    if !ok.as_bool() || ptr.is_null() || len < 4 {
        return None;
    }
    let lang = unsafe { *(ptr as *const u16) };
    let codepage = unsafe { *((ptr as *const u16).add(1)) };
    Some((lang, codepage))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    bound: bool,
    broken: bool,
    revision: u64,
    open_requested: bool,
    /// Set by a host that posts classic messages when `NIM_SETVERSION` was
    /// never seen. Absent on the build that still guesses version 4.
    #[serde(default)]
    unknown_is_classic: bool,
    icons: Vec<HostIcon>,
}

/// What a cheap poll says, without the icon payloads.
pub struct Pulse {
    pub bound: bool,
    pub broken: bool,
    pub revision: u64,
    pub open_requested: bool,
}

/// A live connection to the host inside the current explorer.
pub struct Session {
    pipe: HANDLE,
    pid: u32,
    pub bound: bool,
    revision: u64,
    /// The host posts version-0 callbacks itself when the version was never set.
    pub unknown_is_classic: bool,
    pub icons: Vec<HostIcon>,
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.pipe);
        }
    }
}

impl Session {
    /// Loads the DLL if explorer does not already have it, then connects.
    pub fn attach() -> Option<Self> {
        let pid = explorer_pid()?;
        if !host_loaded(pid) {
            let path = dll_path()?;
            inject(pid, &path).ok()?;
        }
        for _ in 0..40 {
            if let Some(session) = connect(pid) {
                return Some(session);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    pub fn same_explorer(&self) -> bool {
        explorer_pid() == Some(self.pid)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Revision and the "open the panel" flag. Icons stay as they were.
    pub fn pulse(&mut self) -> Option<Pulse> {
        let reply = self.roundtrip(r#"{"op":"poll","full":false}"#)?;
        self.bound = reply.bound && !reply.broken;
        Some(Pulse {
            bound: reply.bound,
            broken: reply.broken,
            revision: reply.revision,
            open_requested: reply.open_requested,
        })
    }

    /// Replaces the cached icon list. Returns whether the revision moved.
    pub fn refresh(&mut self) -> Option<bool> {
        let reply = self.roundtrip(r#"{"op":"poll","full":true}"#)?;
        self.bound = reply.bound && !reply.broken;
        let changed = reply.revision != self.revision;
        self.revision = reply.revision;
        self.icons = reply.icons;
        Some(changed)
    }

    pub fn hover(&mut self, key: &str, enter: bool, anchor: ScreenRect) -> bool {
        let body = serde_json::json!({
            "op": "hover",
            "key": key,
            "enter": enter,
            "anchor": rect_json(anchor),
        });
        self.roundtrip(&body.to_string()).is_some()
    }

    pub fn activate(&mut self, key: &str, right: bool, anchor: ScreenRect) -> bool {
        let body = serde_json::json!({
            "op": "activate",
            "key": key,
            "button": if right { "right" } else { "left" },
            "anchor": rect_json(anchor),
        });
        self.roundtrip(&body.to_string()).is_some()
    }

    /// Ends a hover the panel is no longer showing.
    pub fn release(&mut self) {
        let _ = self.roundtrip(r#"{"op":"own","active":false}"#);
    }

    fn roundtrip(&mut self, body: &str) -> Option<Reply> {
        let mut written = 0u32;
        let sent = unsafe {
            WriteFile(
                self.pipe,
                Some(body.as_bytes()),
                Some(&mut written),
                None,
            )
        };
        if sent.is_err() {
            return None;
        }
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut read = 0u32;
        let got = unsafe { ReadFile(self.pipe, Some(&mut buffer), Some(&mut read), None) };
        if got.is_err() || read == 0 {
            return None;
        }
        let reply: Reply = serde_json::from_slice(&buffer[..read as usize]).ok()?;
        self.unknown_is_classic = reply.unknown_is_classic;
        Some(reply)
    }
}

fn rect_json(anchor: ScreenRect) -> serde_json::Value {
    serde_json::json!({
        "left": anchor.left,
        "top": anchor.top,
        "right": anchor.right,
        "bottom": anchor.bottom,
    })
}

/// CSS pixels of a row, plus the webview's device pixel ratio, mapped onto the
/// screen through the overlay window.
pub fn screen_rect(hwnd: Option<isize>, css_left: f64, css_top: f64, css_right: f64, css_bottom: f64, dpr: f64) -> ScreenRect {
    let scale = if dpr > 0.0 { dpr } else { 1.0 };
    let mut origin = POINT { x: 0, y: 0 };
    if let Some(raw) = hwnd {
        let window = hwnd_from(raw);
        unsafe {
            let _ = ClientToScreen(window, &mut origin);
        }
    }
    ScreenRect {
        left: origin.x + (css_left * scale) as i32,
        top: origin.y + (css_top * scale) as i32,
        right: origin.x + (css_right * scale) as i32,
        bottom: origin.y + (css_bottom * scale) as i32,
    }
}

fn hwnd_from(raw: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(raw as *mut c_void)
}

pub fn explorer_pid() -> Option<u32> {
    let class = wide(island::TASKBAR_CLASS);
    let tray = unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
    let mut pid = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(tray, Some(&mut pid)) };
    if thread == 0 || pid == 0 {
        None
    } else {
        Some(pid)
    }
}

fn dll_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = "trailist_host.dll";
    let beside = dir.join(name);
    if beside.exists() {
        return Some(beside);
    }
    let debug = dir.join("../../host/target/debug").join(name);
    let release = dir.join("../../host/target/release").join(name);
    [debug, release].into_iter().find(|path| path.exists())
}

fn host_loaded(pid: u32) -> bool {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) })
    else {
        return false;
    };
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
    let mut found = false;
    if listed.is_ok() {
        let count = (needed as usize / std::mem::size_of::<windows::Win32::Foundation::HMODULE>())
            .min(modules.len());
        for module in modules.iter().take(count) {
            let mut name = [0u16; 520];
            let length = unsafe { GetModuleFileNameExW(Some(process), Some(*module), &mut name) };
            if length == 0 {
                continue;
            }
            let text = String::from_utf16_lossy(&name[..length as usize]);
            if text.to_ascii_lowercase().ends_with("trailist_host.dll") {
                found = true;
                break;
            }
        }
    }
    unsafe {
        let _ = CloseHandle(process);
    }
    found
}

fn inject(pid: u32, path: &std::path::Path) -> Result<(), String> {
    let access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_READ
        | PROCESS_VM_WRITE;
    let process = unsafe { OpenProcess(access, false, pid) }.map_err(|error| error.to_string())?;
    let wide_path = wide(&path.to_string_lossy());
    let bytes = std::mem::size_of_val(wide_path.as_slice());
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
        unsafe {
            let _ = CloseHandle(process);
        }
        return Err("VirtualAllocEx failed".into());
    }
    let wrote = unsafe {
        WriteProcessMemory(process, remote, wide_path.as_ptr().cast(), bytes, None)
    };
    if wrote.is_err() {
        unsafe {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
        }
        return Err("WriteProcessMemory failed".into());
    }
    let kernel = unsafe { GetModuleHandleW(windows::core::w!("kernel32.dll")) }
        .map_err(|error| error.to_string())?;
    let load = unsafe { GetProcAddress(kernel, windows::core::s!("LoadLibraryW")) }
        .ok_or_else(|| "LoadLibraryW missing".to_string())?;
    let thread = unsafe {
        CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut c_void) -> u32,
            >(load)),
            Some(remote),
            0,
            None,
        )
    };
    let Ok(thread) = thread else {
        unsafe {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
        }
        return Err("CreateRemoteThread failed".into());
    };
    unsafe {
        let _ = WaitForSingleObject(thread, 5000);
    }
    let mut code = 0u32;
    unsafe {
        let _ = GetExitCodeThread(thread, &mut code);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
    }
    if code == 0 {
        Err("LoadLibraryW failed inside explorer".into())
    } else {
        Ok(())
    }
}

fn connect(pid: u32) -> Option<Session> {
    let name = wide(PIPE_NAME);
    let pipe = unsafe {
        windows::Win32::Storage::FileSystem::CreateFileW(
            PCWSTR(name.as_ptr()),
            0x8000_0000 | 0x4000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .ok()?;
    let mode = PIPE_READMODE_MESSAGE;
    unsafe { SetNamedPipeHandleState(pipe, Some(&mode), None, None) }.ok()?;
    Some(Session {
        pipe,
        pid,
        bound: false,
        revision: 0,
        unknown_is_classic: false,
        icons: Vec::new(),
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
