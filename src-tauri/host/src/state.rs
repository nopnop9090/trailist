//! The icon table, the gesture the panel is in the middle of, and whether the
//! panel currently owns the overflow corner.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::parse::{self, NotifyOp};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconRecord {
    pub key: String,
    pub hwnd: isize,
    pub id: u32,
    pub callback: u32,
    pub version: u32,
    /// Whether `NIM_SETVERSION` has been seen. Until then the version is a guess.
    pub version_known: bool,
    pub tip: String,
    pub hidden: bool,
    pub shared_icon: bool,
    pub icon: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

struct Gesture {
    key: String,
    anchor: Anchor,
    until: Instant,
}

pub struct Table {
    icons: BTreeMap<String, IconRecord>,
    revision: u64,
    /// The sink layout parsed at least once, so an empty list is authoritative.
    bound: bool,
    /// A payload with `dwData == 1` did not carry the documented signature.
    broken: bool,
    open_requested: bool,
    resynced: bool,
    client: bool,
    gesture: Option<Gesture>,
}

impl Table {
    fn new() -> Self {
        Self {
            icons: BTreeMap::new(),
            revision: 0,
            bound: false,
            broken: false,
            open_requested: false,
            resynced: false,
            client: false,
            gesture: None,
        }
    }

    pub fn apply(&mut self, op: &NotifyOp, icon: String) {
        self.bound = true;
        let key = self.locate(op);
        match op.message {
            parse::NIM_DELETE => {
                if self.icons.remove(&key).is_some() {
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            parse::NIM_SETVERSION => {
                let version = op.version.unwrap_or(0);
                if let Some(record) = self.icons.get_mut(&key) {
                    record.version = version;
                    record.version_known = true;
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            parse::NIM_SETFOCUS => {}
            parse::NIM_ADD | parse::NIM_MODIFY => {
                let record = self.icons.entry(key.clone()).or_insert_with(|| IconRecord {
                    key: key.clone(),
                    hwnd: op.hwnd,
                    id: op.id,
                    callback: 0,
                    version: 0,
                    version_known: false,
                    tip: String::new(),
                    hidden: false,
                    shared_icon: false,
                    icon: String::new(),
                });
                record.hwnd = op.hwnd;
                record.id = op.id;
                if op.flags & parse::NIF_MESSAGE != 0 {
                    record.callback = op.callback;
                }
                if op.flags & parse::NIF_TIP != 0 && !op.tip.is_empty() {
                    record.tip = op.tip.clone();
                }
                if op.flags & parse::NIF_STATE != 0 {
                    record.hidden = op.hidden;
                    record.shared_icon = op.shared_icon;
                }
                if op.flags & parse::NIF_ICON != 0 && !icon.is_empty() {
                    record.icon = icon;
                }
                if let Some(version) = op.version {
                    if op.message == parse::NIM_ADD && version > 0 {
                        // Some callers write the version into the add. It only
                        // counts once `NIM_SETVERSION` says so; keep the hint.
                        if !record.version_known {
                            record.version = version;
                        }
                    }
                }
                self.revision = self.revision.wrapping_add(1);
            }
            _ => {}
        }
    }

    pub fn note_broken(&mut self) {
        self.broken = true;
    }

    pub fn broken(&self) -> bool {
        self.broken
    }

    pub fn bound(&self) -> bool {
        self.bound && !self.broken
    }

    pub fn mark_resynced(&mut self) {
        self.resynced = true;
    }

    pub fn resynced(&self) -> bool {
        self.resynced
    }

    pub fn set_client(&mut self, connected: bool) {
        self.client = connected;
        if !connected {
            self.gesture = None;
        }
    }

    pub fn client(&self) -> bool {
        self.client
    }

    pub fn request_open(&mut self) {
        if self.client && !self.broken {
            self.open_requested = true;
        }
    }

    pub fn take_open_request(&mut self) -> bool {
        let requested = self.open_requested;
        self.open_requested = false;
        requested
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn icons(&self) -> Vec<IconRecord> {
        self.icons.values().cloned().collect()
    }

    pub fn get(&self, key: &str) -> Option<IconRecord> {
        self.icons.get(key).cloned()
    }

    pub fn begin_gesture(&mut self, key: &str, anchor: Anchor, hold: Duration) {
        self.gesture = Some(Gesture {
            key: key.to_string(),
            anchor,
            until: Instant::now() + hold,
        });
    }

    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    /// The record this message belongs to.
    ///
    /// A GUID wins. A later message that omits the GUID still belongs to that
    /// record when the window and id match, and the other way round: an add
    /// that arrives with a GUID replaces the window-and-id row.
    fn locate(&mut self, op: &NotifyOp) -> String {
        if let Some(guid) = op.guid {
            let key = format!("guid:{}", parse::guid_text(&guid));
            let hwnd_key = format!("hwnd:{}:id:{}", op.hwnd, op.id);
            if hwnd_key != key {
                if let Some(mut previous) = self.icons.remove(&hwnd_key) {
                    previous.key = key.clone();
                    self.icons.entry(key.clone()).or_insert(previous);
                }
            }
            return key;
        }
        self.icons
            .iter()
            .find(|(_, record)| record.hwnd == op.hwnd && record.id == op.id)
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| parse::identity(op))
    }

    /// The rectangle the shell should report for `key`, while a gesture holds it.
    pub fn gesture_anchor(&self, key: &str) -> Option<Anchor> {
        let gesture = self.gesture.as_ref()?;
        if gesture.key != key || Instant::now() > gesture.until {
            return None;
        }
        Some(gesture.anchor)
    }
}

static TABLE: Mutex<Table> = Mutex::new(Table {
    icons: BTreeMap::new(),
    revision: 0,
    bound: false,
    broken: false,
    open_requested: false,
    resynced: false,
    client: false,
    gesture: None,
});

fn with_table<T>(body: impl FnOnce(&mut Table) -> T) -> T {
    let mut guard = TABLE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    body(&mut guard)
}

pub fn apply(op: &NotifyOp, icon: String) {
    with_table(|table| table.apply(op, icon));
}

pub fn note_broken() {
    with_table(|table| table.note_broken());
}

pub fn broken() -> bool {
    with_table(|table| table.broken())
}

pub fn bound() -> bool {
    with_table(|table| table.bound())
}

pub fn mark_resynced() {
    with_table(|table| table.mark_resynced());
}

pub fn resynced() -> bool {
    with_table(|table| table.resynced())
}

pub fn set_client(connected: bool) {
    with_table(|table| table.set_client(connected));
}

pub fn client() -> bool {
    with_table(|table| table.client())
}

pub fn request_open() {
    with_table(|table| table.request_open());
}

pub fn take_open_request() -> bool {
    with_table(|table| table.take_open_request())
}

pub fn snapshot() -> (bool, u64, Vec<IconRecord>) {
    with_table(|table| (table.bound(), table.revision(), table.icons()))
}

pub fn get(key: &str) -> Option<IconRecord> {
    with_table(|table| table.get(key))
}

pub fn begin_gesture(key: &str, anchor: Anchor, hold: Duration) {
    with_table(|table| table.begin_gesture(key, anchor, hold));
}

pub fn end_gesture() {
    with_table(|table| table.end_gesture());
}

pub fn gesture_anchor(key: &str) -> Option<Anchor> {
    with_table(|table| table.gesture_anchor(key))
}

#[allow(dead_code)]
fn _keep_constructor_used() {
    let _ = Table::new;
}
