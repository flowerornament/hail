//! What a command knows about where it runs: the store, the working
//! directory's seat, and the mailboxes this process reads.
//!
//! Identity starts in `seat.rs` (the seat of a directory). This module says
//! who this process is from the filesystem alone; `route.rs` checks that
//! against the live panes when sending, and resolves targets.

use std::fs;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::seat::{self, Addr, Seat, Source};
use crate::store::Store;

pub struct Ctx {
    pub store: Store,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    /// `$TMUX_PANE`: names this process's sub-seat when one exists (see
    /// [`Ctx::mailboxes`]); never identity on its own, since a Codex command
    /// inherits the shared daemon's.
    pub tmux_pane: Option<String>,
    /// Run by Codex. Codex 0.160 sets `CODEX_THREAD_ID` and
    /// `CODEX_SESSION_ID` on every command it runs (checked 2026-10-07).
    pub codex: bool,
    /// Headline cap in characters (`HAIL_ENVELOPE_MAX`, default 400).
    pub max: usize,
}

/// The mailboxes a process reads, from the filesystem alone: its sub-seat
/// when one exists for `$TMUX_PANE` (and it signs as that), then its seat.
/// Reading both keeps a sub-seat's mail reachable after the directory stops
/// being shared, and replies to an old `reply:` keep landing there.
#[derive(Debug, Clone)]
pub struct Boxes {
    /// The mailbox this process signs and replies as.
    pub primary: Addr,
    /// The seat itself, when `primary` is a sub-seat.
    pub seat: Option<Addr>,
}

impl Boxes {
    pub fn iter(&self) -> impl Iterator<Item = &Addr> {
        std::iter::once(&self.primary).chain(&self.seat)
    }
}

impl Ctx {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Self {
            store: Store::from_env(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|h| fs::canonicalize(&h).unwrap_or(h)),
            tmux_pane: var("TMUX_PANE"),
            codex: var("CODEX_THREAD_ID").is_some() || var("CODEX_SESSION_ID").is_some(),
            max: var("HAIL_ENVELOPE_MAX")
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

    /// Like [`Ctx::bind`], but never claims a name: for `doctor`, which is
    /// often run from the wrong directory.
    pub fn check_binding(&self, seat: &Seat) -> Result<()> {
        if seat.source != Source::Env {
            self.store.check_seat(&seat.name, &seat.root)?;
        }
        Ok(())
    }

    pub fn mailboxes(&self, seat: &Seat) -> Boxes {
        let seat_box = Addr::from(seat);
        let sub = self
            .tmux_pane
            .as_deref()
            .filter(|_| !self.codex)
            .map(|p| Addr::sub(&seat.name, p))
            .filter(|a| self.store.seat_dir(a).is_dir());
        match sub {
            Some(sub) => Boxes {
                primary: sub,
                seat: Some(seat_box),
            },
            None => Boxes {
                primary: seat_box,
                seat: None,
            },
        }
    }
}
