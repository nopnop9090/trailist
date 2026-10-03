//! What the watcher thread, the commands and the tray menu share.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::settings::Store;
use crate::types::TrayItem;
use crate::win::island;

/// The live list plus the few flags the watcher needs to tell its own actions
/// apart from the user's.
#[derive(Default)]
pub struct Shared {
    items: Mutex<Vec<TrayItem>>,
    island: Mutex<Option<isize>>,
    /// Where the flyout sat when the panel was opened.
    ///
    /// Kept because a settings change has to re-place an open panel, and the shell
    /// does not need to be asked again for that: where this panel goes is a fact
    /// about the last read, not about the shell right now.
    flyout: Mutex<Option<island::Rect>>,
    /// Whether our panel is on screen. The watcher treats a flyout that appears
    /// while this is set as "the user wants the normal tray back".
    visible: AtomicBool,
    /// The open list came from the host, so clicks name a registration.
    direct: AtomicBool,
    requests: Mutex<Option<Sender<Request>>>,
}

impl Shared {
    pub fn set_items(&self, items: Vec<TrayItem>, island_window: isize, flyout: island::Rect) {
        *self.items.lock() = items;
        *self.island.lock() = Some(island_window);
        *self.flyout.lock() = Some(flyout);
    }

    pub fn items(&self) -> Vec<TrayItem> {
        self.items.lock().clone()
    }

    pub fn item_at(&self, index: usize) -> Option<TrayItem> {
        self.items.lock().get(index).cloned()
    }

    pub fn island(&self) -> Option<isize> {
        *self.island.lock()
    }

    /// Where the flyout sat when the panel was opened, for re-placing it.
    pub fn flyout(&self) -> Option<island::Rect> {
        *self.flyout.lock()
    }

    pub fn set_visible(&self, value: bool) {
        self.visible.store(value, Ordering::SeqCst);
    }

    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn set_direct(&self, value: bool) {
        self.direct.store(value, Ordering::SeqCst);
    }

    pub fn direct(&self) -> bool {
        self.direct.load(Ordering::SeqCst)
    }

    /// Handed to the watcher once at startup; every later request goes through
    /// this channel so the shell is only ever touched from that one thread.
    pub fn attach(&self, sender: Sender<Request>) {
        *self.requests.lock() = Some(sender);
    }

    pub fn send(&self, request: Request) -> bool {
        self.requests
            .lock()
            .as_ref()
            .map(|sender| sender.send(request).is_ok())
            .unwrap_or(false)
    }
}

/// Work that has to happen on the watcher thread, because that is where UI
/// Automation lives and where a click that blocks for a third of a second is
/// allowed to block.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    /// Show or hide the list. With the host attached this does not touch the chevron.
    Toggle,
    /// Activate the icon at `index`. `anchor` is the row, in screen pixels.
    Click {
        index: usize,
        right: bool,
        anchor: crate::win::host::ScreenRect,
    },
    /// Hover entered or left the row. The panel stays open.
    Hover {
        index: usize,
        enter: bool,
        anchor: crate::win::host::ScreenRect,
    },
}

/// Everything Tauri manages for us.
pub struct AppState {
    pub shared: Arc<Shared>,
    pub store: Arc<Store>,
}

impl AppState {
    pub fn prefs(&self) -> crate::types::Prefs {
        self.store.prefs()
    }
}