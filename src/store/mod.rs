//! State under `$XDG_STATE_HOME/hail`. No daemon, no database: the
//! files are the state, and every write that others read is a rename.

pub mod gc;
pub mod ids;
pub mod mailbox;
pub mod message;
pub mod records;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::seat::Addr;
use ids::Id;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$XDG_STATE_HOME/hail`, default `~/.local/state/hail`.
    pub fn from_env() -> Self {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from(".local/state"));
        Self::at(base.join("hail"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn seats_dir(&self) -> PathBuf {
        self.root.join("seats")
    }

    /// A mailbox's directory: `seats/<seat>` or `seats/<seat>@<pane>`.
    pub fn seat_dir(&self, addr: &Addr) -> PathBuf {
        self.seats_dir().join(addr.to_string())
    }

    pub fn mailbox(&self, addr: &Addr) -> mailbox::Mailbox {
        mailbox::Mailbox::new(self.seat_dir(addr))
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

    /// 0.3 state not yet migrated (`hail migrate` imports it).
    pub fn legacy_present(&self) -> bool {
        self.root.join("inbox").is_dir()
    }

    /// Every mailbox with a directory, sorted.
    pub fn mailboxes(&self) -> Vec<Addr> {
        list_names(&self.seats_dir())
            .iter()
            .map(|n| Addr::parse(n))
            .collect()
    }

    /// Bind a seat name to its root on first use; a different root claiming
    /// the same name is refused (two repos named `foo`).
    pub fn bind_seat(&self, name: &str, root: &Path) -> Result<()> {
        if self.check_seat(name, root)? {
            return Ok(());
        }
        let dir = self.seats_dir().join(name);
        fs::create_dir_all(&dir).map_err(Error::at(&dir))?;
        write_atomic(
            &dir.join("root"),
            format!("{}\n", root.display()).as_bytes(),
        )
    }

    /// Whether `name` is already bound to `root` (true), not bound yet
    /// (false), or bound elsewhere (an error naming the fix). Writes nothing.
    pub fn check_seat(&self, name: &str, root: &Path) -> Result<bool> {
        let Some(have) = self.seat_root(name) else {
            return Ok(false);
        };
        if have == root {
            return Ok(true);
        }
        Err(Error::Seat(format!(
            "seat {name} is bound to {}; add .hail-seat with another name in {}",
            have.display(),
            root.display()
        )))
    }

    pub fn seat_root(&self, name: &str) -> Option<PathBuf> {
        fs::read_to_string(self.seats_dir().join(name).join("root"))
            .ok()
            .map(|s| PathBuf::from(s.trim_end()))
    }
}

/// Where a message stands, as `hail sent` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// No message with this id (or it was never indexed).
    Unknown,
    /// In the recipient's inbox, not yet claimed.
    Delivered,
    /// Claimed: injected by a hook, read with `hail inbox`, or inline.
    Received(mailbox::Receipt),
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown"),
            Self::Delivered => f.write_str("delivered"),
            Self::Received(r) => r.fmt(f),
        }
    }
}

impl Store {
    /// A message by id, wherever it is: its seat's mailbox through the
    /// index, else the gc archive (`archive/index`).
    pub fn find(&self, id: &Id) -> Option<mailbox::Found> {
        if let Some(seat) = ids::lookup(self, id)
            && let Some(f) = self.mailbox(&seat).find(id)
        {
            return Some(f);
        }
        let index = fs::read_to_string(self.archive_dir().join("index")).ok()?;
        let line = index
            .lines()
            .find(|l| l.split(' ').next() == Some(id.as_str()))?;
        let mut f = line.split(' ').skip(1);
        let (month, seat) = (f.next()?, f.next()?);
        mailbox::Mailbox::new(self.archive_dir().join(month).join(seat)).find(id)
    }

    pub fn status(&self, id: &Id) -> Status {
        match self.find(id) {
            Some(mailbox::Found {
                receipt: Some(r), ..
            }) => Status::Received(r),
            Some(_) => Status::Delivered,
            None => Status::Unknown,
        }
    }
}

/// Write via a temp file in the same directory and rename into place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(Error::at(dir))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut f = fs::File::create(&tmp).map_err(Error::at(&tmp))?;
    f.write_all(bytes).map_err(Error::at(&tmp))?;
    fs::rename(&tmp, path).map_err(Error::at(path))
}

/// File names in a directory (not dot files), sorted; empty when missing.
pub fn list_names(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(std::result::Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}
