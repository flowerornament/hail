//! What a command knows about where it runs: the store, the working
//! directory's seat, and (for verbs that need tmux) the panes and their seats.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::seat::{self, Seat, Source};
use crate::store::Store;
use crate::transport::tmux::{Pane, Tmux};

pub struct Ctx {
    pub store: Store,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    /// `$TMUX_PANE`. Trusted only after a check against the pane's own
    /// directory (sub-seats); a Codex command inherits the daemon's.
    pub tmux_pane: Option<String>,
    /// Headline cap in characters (`HAIL_ENVELOPE_MAX`, default 400).
    pub max: usize,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| fs::canonicalize(&h).unwrap_or(h));
        Ctx {
            store: Store::from_env(),
            cwd,
            home,
            tmux_pane: std::env::var("TMUX_PANE").ok().filter(|p| !p.is_empty()),
            max: std::env::var("HAIL_ENVELOPE_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n >= 20)
                .unwrap_or(400),
        }
    }

    /// The seat of the working directory. `HAIL_SEAT` counts only where the
    /// directory names no seat, so a stale inherited value cannot override one.
    pub fn seat_here(&self) -> Result<Option<Seat>> {
        if let Some(s) = seat::seat_of(&self.cwd, self.home.as_deref())? {
            return Ok(Some(s));
        }
        match std::env::var("HAIL_SEAT").ok().filter(|v| !v.is_empty()) {
            Some(name) if seat::valid_name(&name) && !seat::RESERVED.contains(&name.as_str()) => {
                Ok(Some(Seat {
                    name,
                    root: self.cwd.clone(),
                    source: Source::Env,
                }))
            }
            Some(name) => Err(Error::Seat(format!(
                "HAIL_SEAT={name} is not a plain seat name"
            ))),
            None => Ok(None),
        }
    }

    /// The seat here, bound to its root, or exit 3 with the fix.
    pub fn require_seat(&self) -> Result<Seat> {
        let seat = self.seat_here()?.ok_or_else(|| {
            Error::Seat(format!(
                "no seat here: {} is not in a jj workspace or git repo; run hail from your workspace, or add a .hail-seat file naming it",
                self.cwd.display()
            ))
        })?;
        self.bind(&seat)?;
        Ok(seat)
    }

    /// Bind a directory-derived seat to its root (first use claims the name).
    /// A `HAIL_SEAT` seat has no root to bind: binding it to whichever
    /// directory it was first used from would refuse every other one.
    pub fn bind(&self, seat: &Seat) -> Result<()> {
        if seat.source == Source::Env {
            return Ok(());
        }
        self.store.bind_seat(&seat.name, &seat.root)
    }

    /// A command Codex runs carries the daemon's `TMUX_PANE`, which may name
    /// another agent's pane, so Codex never reads or signs as a sub-seat.
    pub fn is_codex(&self) -> bool {
        ["CODEX_THREAD_ID", "CODEX_SESSION_ID"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()))
    }

    /// This process's mailboxes, from the filesystem alone (hooks, inbox,
    /// brief): its sub-seat when one exists for `$TMUX_PANE`, then the seat.
    /// Reading both keeps a sub-seat's mail reachable after the directory
    /// stops being shared and replies to an old `reply:` keep landing there.
    pub fn my_mailboxes(&self, seat: &Seat) -> Vec<String> {
        let mut boxes = Vec::with_capacity(2);
        if let (false, Some(p)) = (self.is_codex(), &self.tmux_pane) {
            let sub = seat::sub_seat(&seat.name, p);
            if self.store.seat_dir(&sub).is_dir() {
                boxes.push(sub);
            }
        }
        boxes.push(seat.name.clone());
        boxes
    }

    /// The mailbox this process signs as: its sub-seat when it has one.
    pub fn my_mailbox(&self, seat: &Seat) -> String {
        self.my_mailboxes(seat).swap_remove(0)
    }
}

/// The panes on the server, each mapped to the seat of its directory.
pub struct PaneMap {
    pub panes: Vec<Pane>,
    seats: HashMap<PathBuf, Option<String>>,
}

impl PaneMap {
    pub fn load(tmux: &Tmux, home: Option<&Path>) -> Result<PaneMap> {
        let panes = tmux.panes()?;
        let mut seats = HashMap::new();
        for p in &panes {
            seats
                .entry(p.path.clone())
                .or_insert_with(|| seat::seat_of(&p.path, home).ok().flatten().map(|s| s.name));
        }
        Ok(PaneMap { panes, seats })
    }

    pub fn seat_of(&self, pane: &Pane) -> Option<&str> {
        self.seats.get(&pane.path).and_then(|s| s.as_deref())
    }

    pub fn find(&self, id: &str) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn in_seat(&self, seat: &str) -> Vec<&Pane> {
        self.panes
            .iter()
            .filter(|p| self.seat_of(p) == Some(seat))
            .collect()
    }

    pub fn agents_in(&self, seat: &str) -> Vec<&Pane> {
        self.in_seat(seat)
            .into_iter()
            .filter(|p| p.agent.is_some())
            .collect()
    }

    pub fn seat_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.seats.values().flatten().cloned().collect();
        v.sort();
        v.dedup();
        v
    }
}

/// A Claude pane may hold a sub-seat; a Codex pane may not, because its
/// commands carry the daemon's TMUX_PANE, not their own.
pub fn sub_seat_eligible(p: &Pane) -> bool {
    p.agent.as_deref().is_some_and(|a| a != "codex")
}

/// Keep `seats/<seat>/shared` in step with the panes seen: present, listing
/// them, when more than one agent works in the seat's directory.
pub fn note_sharing(store: &Store, seat: &str, agents: &[&Pane]) {
    let marker = store.seat_dir(seat).join("shared");
    if agents.len() > 1 {
        let ids: Vec<&str> = agents.iter().map(|p| p.id.as_str()).collect();
        let _ = crate::store::write_atomic(&marker, format!("{}\n", ids.join(" ")).as_bytes());
    } else if marker.exists() {
        let _ = fs::remove_file(marker);
    }
}

/// `%5`, `sess:1.2`, `3`: a tmux target rather than a seat name.
pub fn looks_like_pane(arg: &str) -> bool {
    (arg.starts_with('%') && arg[1..].chars().all(|c| c.is_ascii_digit()) && arg.len() > 1)
        || arg.contains(':')
        || (!arg.is_empty() && arg.chars().all(|c| c.is_ascii_digit()))
}
