//! Identity is the seat: the workspace an agent works in, derived from a
//! directory and nothing else. Process trees and an inherited `TMUX_PANE`
//! each named the wrong pane, and so misrouted mail (murail-4vc8v,
//! murail-m65jq, and Codex's shared app-server, a child of whichever pane
//! started it); the working directory survives all of them.
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
/// Verb names cannot be seats: `hail <verb>` would never reach them.
pub use crate::cli::VERBS as RESERVED;

pub fn valid_name(name: &str) -> bool {
    path_safe(name)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A part of an address becomes one directory name under `seats/`, so it
/// must name exactly one entry there: not empty, `.` or `..`, and without
/// `/` or NUL. A target of `..` once wrote mail outside the store.
pub fn path_safe(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && !part.contains(['/', '\0'])
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
    /// `hail` or `hail@%28`. The one place a string from outside (argv, the
    /// environment, a file) becomes an address: `None` unless every part is
    /// [`path_safe`].
    pub fn parse(s: &str) -> Option<Self> {
        let addr = match s.split_once('@') {
            Some((seat, pane)) => Self::Sub {
                seat: seat.to_string(),
                pane: pane.to_string(),
            },
            None => Self::Seat(s.to_string()),
        };
        addr.is_path_safe().then_some(addr)
    }

    /// Every part names one directory entry; see [`path_safe`].
    pub fn is_path_safe(&self) -> bool {
        match self {
            Self::Seat(s) => path_safe(s),
            Self::Sub { seat, pane } => path_safe(seat) && path_safe(pane),
        }
    }

    /// From parts already checked: a seat name from [`seat_of`] and a pane
    /// id from tmux.
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

/// What a target or a `reply:` value names. `/` is split first, then each
/// side goes through [`Addr::parse`] or the name rules, so nothing here can
/// name a directory outside the store.
///
/// - `murail-1b`, `hail@%28`: a mailbox.
/// - `murail-1b/%7`: a pane in that seat, the form `from:` prints, so a
///   `from:` value pasted as a target works.
/// - `murail-1b/recip-consumer`, `hail@%28/scout`: a sub-agent. It has no
///   mailbox of its own: mail goes to the parent's, marked `for: <name>`,
///   and the parent relays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    Mailbox(Addr),
    Pane { seat: String, pane: String },
    Agent { parent: Addr, name: String },
}

impl Address {
    pub fn parse(s: &str) -> Option<Self> {
        let Some((left, right)) = s.split_once('/') else {
            return Addr::parse(s).map(Self::Mailbox);
        };
        if let Some(digits) = right.strip_prefix('%') {
            let pane_ok = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
            return match Addr::parse(left)? {
                Addr::Seat(seat) if pane_ok => Some(Self::Pane {
                    seat,
                    pane: right.to_string(),
                }),
                _ => None,
            };
        }
        let parent = Addr::parse(left)?;
        valid_name(right).then(|| Self::Agent {
            parent,
            name: right.to_string(),
        })
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
            assert_eq!(Addr::parse(s).unwrap().to_string(), s);
        }
        assert_eq!(Addr::parse("hail@%28").unwrap().seat(), "hail");
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

    #[test]
    fn an_address_part_names_exactly_one_directory_entry() {
        for bad in [
            "", ".", "..", "a/b", "../x", "a\0b", "hail@..", "@%1", "hail@",
        ] {
            assert_eq!(Addr::parse(bad), None, "{bad:?}");
        }
        for good in ["hail", ".nix-config", "legacy-%1", "hail@%28", "..."] {
            assert!(Addr::parse(good).is_some(), "{good:?}");
        }
        assert!(!valid_name("..") && !valid_name(".") && valid_name(".nix-config"));
    }

    #[test]
    fn addresses_split_on_slash_first() {
        let agent = |p: &str, n: &str| Address::Agent {
            parent: Addr::parse(p).unwrap(),
            name: n.into(),
        };
        assert_eq!(
            Address::parse("murail-2b/recip-consumer"),
            Some(agent("murail-2b", "recip-consumer"))
        );
        assert_eq!(
            Address::parse("hail@%28/scout"),
            Some(agent("hail@%28", "scout"))
        );
        assert_eq!(
            Address::parse("murail-1a/%5"),
            Some(Address::Pane {
                seat: "murail-1a".into(),
                pane: "%5".into()
            })
        );
        assert_eq!(
            Address::parse("hail@%28"),
            Some(Address::Mailbox(Addr::parse("hail@%28").unwrap()))
        );
        // '@' inside the name side never makes a sub-seat of the parent.
        for bad in [
            "a/b@c",
            "a/b/c",
            "../x",
            "x/..",
            "x/.",
            "a/",
            "/b",
            "a/%",
            "a/%x",
            "hail@%28/%5",
            "a/b c",
        ] {
            assert_eq!(Address::parse(bad), None, "{bad:?}");
        }
    }
}
