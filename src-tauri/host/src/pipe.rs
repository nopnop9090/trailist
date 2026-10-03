//! One client, the TrayList process. Requests are JSON, one message at a time.
//!
//! The pipe is created with an ACL for the current user only. Anything else
//! that can talk to explorer is not invited to drive tray gestures.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

use crate::log;
use crate::state::{self, Anchor, IconRecord};
use crate::tray;

const PIPE_NAME: &str = "\\\\.\\pipe\\TrayList.Host";

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    Poll {
        #[serde(default)]
        full: bool,
    },
    Hover {
        key: String,
        enter: bool,
        anchor: WireRect,
    },
    Activate {
        key: String,
        button: String,
        anchor: WireRect,
    },
    Own { active: bool },
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct WireRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl From<WireRect> for Anchor {
    fn from(rect: WireRect) -> Self {
        Anchor {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    ok: bool,
    bound: bool,
    broken: bool,
    revision: u64,
    open_requested: bool,
    flyout_visible: bool,
    /// This host posts classic callbacks when `NIM_SETVERSION` was never seen.
    unknown_is_classic: bool,
    icons: Vec<IconRecord>,
}

pub fn serve() {
    log::line("pipe server starting");
    loop {
        let Ok(pipe) = create_pipe() else {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        };
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if let Err(error) = connected {
            log::line(&format!("ConnectNamedPipe: {error}"));
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(pipe) };
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        log::line("client connected");
        state::set_client(true);
        ensure_resync();
        while let Some(request) = read_request(pipe) {
            let reply = handle(request);
            if write_reply(pipe, &reply).is_err() {
                break;
            }
        }
        state::set_client(false);
        log::line("client disconnected");
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = windows::Win32::Foundation::CloseHandle(pipe);
        }
    }
}

fn ensure_resync() {
    if state::broken() || state::resynced() {
        return;
    }
    // One broadcast fills the table with icons that were registered before
    // the subclass existed. Apps already treat `TaskbarCreated` as "add your
    // icon again". Doing it twice would make them add duplicates.
    tray::broadcast_taskbar_created();
    std::thread::sleep(Duration::from_millis(600));
    state::mark_resynced();
}

fn handle(request: Request) -> Reply {
    match request {
        Request::Poll { full } => reply(true, full, true),
        Request::Own { active } => {
            if !active {
                state::end_gesture();
            }
            reply(true, false, false)
        }
        Request::Hover { key, enter, anchor } => {
            if let Some(record) = state::get(&key) {
                tray::deliver(&record, if enter { "enter" } else { "leave" }, anchor.into());
            }
            reply(true, false, false)
        }
        Request::Activate { key, button, anchor } => {
            if let Some(record) = state::get(&key) {
                let kind = if button == "right" { "right" } else { "left" };
                tray::deliver(&record, kind, anchor.into());
            }
            reply(true, false, false)
        }
    }
}

fn reply(ok: bool, full: bool, take_open: bool) -> Reply {
    let (bound, revision, icons) = if full {
        state::snapshot()
    } else {
        let (bound, revision, _) = state::snapshot();
        (bound, revision, Vec::new())
    };
    Reply {
        ok,
        bound,
        broken: state::broken(),
        revision,
        open_requested: if take_open {
            state::take_open_request()
        } else {
            false
        },
        flyout_visible: tray::flyout_visible(),
        unknown_is_classic: true,
        icons,
    }
}

fn create_pipe() -> windows::core::Result<windows::Win32::Foundation::HANDLE> {
    let name = wide(PIPE_NAME);
    // Explorer's own token DACL. A hand-built SDDL kept the instance from
    // appearing for the logged-on user on this build; the process token is
    // already that user.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            1 << 20,
            1 << 20,
            0,
            None,
        )
    };
    if pipe.is_invalid() {
        let err = windows::core::Error::from_thread();
        log::line(&format!("CreateNamedPipe failed: {err}"));
        Err(err)
    } else {
        log::line("pipe listening");
        Ok(pipe)
    }
}

fn read_request(pipe: windows::Win32::Foundation::HANDLE) -> Option<Request> {
    let mut buffer = vec![0u8; 1 << 16];
    let mut read = 0u32;
    let ok = unsafe { ReadFile(pipe, Some(&mut buffer), Some(&mut read), None) };
    if ok.is_err() || read == 0 {
        return None;
    }
    serde_json::from_slice(&buffer[..read as usize]).ok()
}

fn write_reply(pipe: windows::Win32::Foundation::HANDLE, reply: &Reply) -> Result<(), ()> {
    let bytes = serde_json::to_vec(reply).map_err(|_| ())?;
    let mut written = 0u32;
    unsafe { WriteFile(pipe, Some(&bytes), Some(&mut written), None) }.map_err(|_| ())?;
    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

