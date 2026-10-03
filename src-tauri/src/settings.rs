//! Preferences on disk.
//!
//! Portable first: a `config/` folder beside the executable wins, `%APPDATA%`
//! is the fallback for the case where the executable sits somewhere read-only.
//! The file stays hand-editable, like the rest of the machine's tooling.

use std::path::{Path, PathBuf};

use anyhow::Context;
use parking_lot::RwLock;

use crate::types::Prefs;

pub const FILE: &str = "settings.json";

pub struct Store {
    directory: PathBuf,
    cache: RwLock<Prefs>,
}

impl Store {
    pub fn open() -> Self {
        let directory = resolve_directory();
        let cache = RwLock::new(read(&directory.join(FILE)).unwrap_or_default());
        Self { directory, cache }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn prefs(&self) -> Prefs {
        self.cache.read().clone()
    }

    /// Writes the file and only then updates the cache, so a failed write cannot
    /// leave the running app believing in settings that are not on disk.
    pub fn write(&self, prefs: Prefs) -> anyhow::Result<()> {
        let path = self.directory.join(FILE);
        let json = serde_json::to_string_pretty(&prefs)?;
        std::fs::write(&path, json).with_context(|| format!("cannot write {}", path.display()))?;
        *self.cache.write() = prefs;
        Ok(())
    }
}

fn read(path: &Path) -> Option<Prefs> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn resolve_directory() -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let candidate = parent.join("config");
            if std::fs::create_dir_all(&candidate).is_ok() {
                return candidate;
            }
        }
    }

    let fallback = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("TrayList");
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}