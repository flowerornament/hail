//! State under `$XDG_STATE_HOME/hail` (spec §6). No daemon, no database: the
//! files are the state, and every write that others read is a rename.

pub mod ids;
pub mod mailbox;
pub mod message;
pub mod records;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    /// `$XDG_STATE_HOME/hail`, default `~/.local/state/hail`.
    pub fn from_env() -> Store {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from(".local/state"));
        Store::at(base.join("hail"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn seats_dir(&self) -> PathBuf {
        self.root.join("seats")
    }

    pub fn seat_dir(&self, seat: &str) -> PathBuf {
        self.seats_dir().join(seat)
    }

    pub fn mailbox(&self, seat: &str) -> mailbox::Mailbox {
        mailbox::Mailbox::new(self.seat_dir(seat))
    }

    pub fn ids_dir(&self) -> PathBuf {
        self.root.join("ids")
    }

    pub fn holds_dir(&self) -> PathBuf {
        self.root.join("holds")
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    /// 0.3 state not yet migrated (spec §11.1).
    pub fn legacy_present(&self) -> bool {
        self.root.join("inbox").is_dir()
    }

    /// Every seat directory name, sorted.
    pub fn seat_names(&self) -> Vec<String> {
        list_names(&self.seats_dir())
    }

    /// Bind a seat name to its root on first use; a different root claiming
    /// the same name is refused (two repos named `foo`).
    pub fn bind_seat(&self, name: &str, root: &Path) -> Result<()> {
        let dir = self.seat_dir(name);
        let file = dir.join("root");
        let want = root.to_string_lossy();
        match fs::read_to_string(&file) {
            Ok(have) if have.trim_end() == want => Ok(()),
            Ok(have) => Err(Error::Seat(format!(
                "seat {name} is bound to {}; add .hail-seat with another name in {want}",
                have.trim_end()
            ))),
            Err(_) => {
                fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
                write_atomic(&file, format!("{want}\n").as_bytes())
            }
        }
    }

    pub fn seat_root(&self, name: &str) -> Option<PathBuf> {
        fs::read_to_string(self.seat_dir(name).join("root"))
            .ok()
            .map(|s| PathBuf::from(s.trim_end()))
    }
}

/// Write via a temp file in the same directory and rename into place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut f = fs::File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    f.write_all(bytes).map_err(|e| Error::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

/// File names in a directory (not dot files), sorted; empty when missing.
pub fn list_names(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}
