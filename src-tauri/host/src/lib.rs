//! TrayList's seat inside `explorer.exe`.
//!
//! The panel process loads this DLL into the shell. From there it watches
//! `Shell_NotifyIcon` traffic, posts hover and click notifications to the
//! owning windows, and keeps the stock overflow flyout from painting. A
//! failure to recognise the sink leaves the DLL loaded and quiet: the panel
//! then keeps driving the flyout the way it did before this crate existed.

mod icon;
mod log;
mod parse;
mod pipe;
mod state;
mod tray;

use std::ffi::c_void;
use std::time::Duration;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::System::Threading::{CreateThread, THREAD_CREATION_FLAGS};

/// Saved so a hook can name this DLL as its module. Set only from `DllMain`.
static mut MODULE: HINSTANCE = HINSTANCE(std::ptr::null_mut());

#[no_mangle]
pub unsafe extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> i32 {
    if reason == log::ATTACH {
        let _ = DisableThreadLibraryCalls(module.into());
        MODULE = module;
        tray::remember_module(module.0 as isize);
        // Nothing else is legal on the loader lock. The worker thread does
        // the subclass, the pipe and the flyout watch.
        let _ = CreateThread(
            None,
            0,
            Some(worker),
            None,
            THREAD_CREATION_FLAGS(0),
            None,
        );
    }
    1
}

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    let _ = std::panic::catch_unwind(|| {
        log::line("host thread started");
        if !tray::subclass_tray() {
            log::line("tray subclass failed; staying inert");
            return;
        }
        if log::spy_path().exists() {
            std::thread::spawn(|| tray::spy_for(Duration::from_secs(8)));
        }
        std::thread::spawn(flyout_watch);
        pipe::serve();
    });
    0
}

fn flyout_watch() {
    loop {
        let _ = std::panic::catch_unwind(|| {
            if state::client() && !state::broken() {
                tray::watch_flyout();
            }
        });
        std::thread::sleep(Duration::from_millis(30));
    }
}
