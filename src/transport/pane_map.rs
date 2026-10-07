//! The panes on the server, each mapped to the seat of its directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::tmux::{Pane, Tmux};
use crate::error::Result;
use crate::seat;

pub struct PaneMap {
    pub panes: Vec<Pane>,
    /// `seat_of` per directory, memoised: many panes share a directory.
    seats: HashMap<PathBuf, Option<String>>,
}

impl PaneMap {
    pub fn load(tmux: &Tmux, home: Option<&Path>) -> Result<Self> {
        let panes = tmux.panes()?;
        let mut seats = HashMap::new();
        for p in &panes {
            seats
                .entry(p.path.clone())
                .or_insert_with(|| seat::seat_of(&p.path, home).ok().flatten().map(|s| s.name));
        }
        Ok(Self { panes, seats })
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
