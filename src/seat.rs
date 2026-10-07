//! Identity is the seat: the workspace an agent works in, derived from a
//! directory and nothing else. Process trees and an inherited `TMUX_PANE` named
//! the wrong pane three times in 0.3 (murail-4vc8v, murail-m65jq, the Codex
//! app-server as a pane's child); the working directory survives all of them.
//!
//! Identity has three steps: this module names a directory's seat;
//! `ctx.rs` picks the mailboxes this process reads and signs as; `route.rs`
//! resolves the other end, a send's target or a pane to drive.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub name: String,
    pub root: PathBuf,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    SeatFile,
    Jj,
    Git,
    Env,
}

impl Source {
    pub fn describe(self) -> &'static str {
        match self {
            Self::SeatFile => ".hail-seat",
            Self::Jj => "jj workspace root",
            Self::Git => "git root",
            Self::Env => "HAIL_SEAT (no workspace here)",
        }
    }
}

/// Names that would read as a verb in `hail <seat> ...`.
pub const RESERVED: &[&str] = &[
    "send", "message", "msg", "sent", "await", "deliver", "brief", "inbox", "show", "whoami",
    "seats", "list", "read", "type", "keys", "setup", "doctor", "migrate", "gc", "help", "version",
    "name", "hello", "who", "resolve", "id",
];

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The seat of a directory: walk up, stopping before `home` (never a seat
/// itself). The first `.hail-seat` file names it; else the first `.jj`
/// directory or `.git` (a file in worktrees and submodules) gives its basename.
pub fn seat_of(dir: &Path, home: Option<&Path>) -> Result<Option<Seat>> {
    for anc in dir.ancestors() {
        if home == Some(anc) {
            break;
        }
        let seat_file = anc.join(".hail-seat");
        if seat_file.is_file() {
            let name = fs::read_to_string(&seat_file).map_err(Error::at(&seat_file))?;
            return checked(name.trim(), anc, Source::SeatFile).map(Some);
        }
        let source = if anc.join(".jj").is_dir() {
            Source::Jj
        } else if anc.join(".git").exists() {
            Source::Git
        } else {
            continue;
        };
        let name = anc.file_name().and_then(|n| n.to_str()).unwrap_or("");
        return checked(name, anc, source).map(Some);
    }
    Ok(None)
}

fn checked(name: &str, root: &Path, source: Source) -> Result<Seat> {
    let fix = format!(
        "write a plain name ([A-Za-z0-9._-]) to {}/.hail-seat",
        root.display()
    );
    if !valid_name(name) {
        return Err(Error::Seat(format!(
            "seat name '{name}' at {} is not plain; {fix}",
            root.display()
        )));
    }
    if RESERVED.contains(&name) {
        return Err(Error::Seat(format!(
            "seat name '{name}' is a hail verb; {fix}"
        )));
    }
    Ok(Seat {
        name: name.to_string(),
        root: root.to_path_buf(),
        source,
    })
}

/// A mailbox: a seat, or one Claude pane's sub-seat in a directory several
/// agents share. Written `seat` or `seat@%pane`, in directory names, the
/// id index and `reply:`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Addr {
    Seat(String),
    Sub { seat: String, pane: String },
}

impl Addr {
    /// `hail` or `hail@%28`.
    pub fn parse(s: &str) -> Self {
        match s.split_once('@') {
            Some((seat, pane)) => Self::Sub {
                seat: seat.to_string(),
                pane: pane.to_string(),
            },
            None => Self::Seat(s.to_string()),
        }
    }

    pub fn sub(seat: &str, pane: &str) -> Self {
        Self::Sub {
            seat: seat.to_string(),
            pane: pane.to_string(),
        }
    }

    /// The seat this mailbox belongs to.
    pub fn seat(&self) -> &str {
        match self {
            Self::Seat(s) | Self::Sub { seat: s, .. } => s,
        }
    }
}

impl std::fmt::Display for Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Seat(s) => f.write_str(s),
            Self::Sub { seat, pane } => write!(f, "{seat}@{pane}"),
        }
    }
}

impl From<&Seat> for Addr {
    fn from(s: &Seat) -> Self {
        Self::Seat(s.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("home/code/murail-1b/.jj")).unwrap();
        fs::create_dir_all(t.path().join("home/code/murail-1b/src/deep")).unwrap();
        fs::create_dir_all(t.path().join("home/code/wt")).unwrap();
        fs::write(t.path().join("home/code/wt/.git"), "gitdir: elsewhere").unwrap();
        fs::create_dir_all(t.path().join("home/code/named/sub")).unwrap();
        fs::write(t.path().join("home/code/named/.hail-seat"), "desk-a\n").unwrap();
        fs::create_dir_all(t.path().join("home/.git")).unwrap();
        fs::create_dir_all(t.path().join("home/stray")).unwrap();
        t
    }

    #[test]
    fn addresses_round_trip() {
        for s in ["hail", "hail@%28", "legacy-%14"] {
            assert_eq!(Addr::parse(s).to_string(), s);
        }
        assert_eq!(Addr::parse("hail@%28").seat(), "hail");
    }

    #[test]
    fn derives_seats() {
        let t = tree();
        let home = t.path().join("home");
        let s = seat_of(&home.join("code/murail-1b/src/deep"), Some(&home))
            .unwrap()
            .unwrap();
        assert_eq!((s.name.as_str(), s.source), ("murail-1b", Source::Jj));
        let s = seat_of(&home.join("code/wt"), Some(&home))
            .unwrap()
            .unwrap();
        assert_eq!((s.name.as_str(), s.source), ("wt", Source::Git));
        let s = seat_of(&home.join("code/named/sub"), Some(&home))
            .unwrap()
            .unwrap();
        assert_eq!((s.name.as_str(), s.source), ("desk-a", Source::SeatFile));
        // A dotfiles repo at $HOME never makes stray directories a seat.
        assert_eq!(seat_of(&home.join("stray"), Some(&home)).unwrap(), None);
    }

    #[test]
    fn refuses_verbs_and_odd_names() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("inbox/.git")).unwrap();
        assert!(matches!(
            seat_of(&t.path().join("inbox"), None),
            Err(Error::Seat(_))
        ));
        fs::create_dir_all(t.path().join("my repo/.git")).unwrap();
        assert!(matches!(
            seat_of(&t.path().join("my repo"), None),
            Err(Error::Seat(_))
        ));
    }
}
