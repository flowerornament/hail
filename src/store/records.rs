//! Standing state as small header-only records:
//! `seats/<seat>/owed/<id>` (obligations on the seat), `holds/<id>` (global),
//! `seats/<seat>/pending/<id>` (my sends with no receipt yet).

use std::fs;
use std::path::PathBuf;

use super::message::Message;
use super::{Store, list_names, write_atomic};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Owed,
    Holds,
    Pending,
}

impl Store {
    fn records_dir(&self, kind: Kind, seat: &str) -> PathBuf {
        match kind {
            Kind::Owed => self.seat_dir(seat).join("owed"),
            Kind::Pending => self.seat_dir(seat).join("pending"),
            Kind::Holds => self.holds_dir(),
        }
    }

    pub fn record_path(&self, kind: Kind, seat: &str, id: &str) -> PathBuf {
        self.records_dir(kind, seat).join(id)
    }

    pub fn put_record(&self, kind: Kind, seat: &str, id: &str, rec: &Message) -> Result<()> {
        write_atomic(
            &self.record_path(kind, seat, id),
            rec.render_record().as_bytes(),
        )
    }

    pub fn has_record(&self, kind: Kind, seat: &str, id: &str) -> bool {
        !id.contains('/') && self.record_path(kind, seat, id).is_file()
    }

    pub fn remove_record(&self, kind: Kind, seat: &str, id: &str) -> Result<()> {
        let p = self.record_path(kind, seat, id);
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(&p, e)),
        }
    }

    /// Records newest first (ids sort by time).
    pub fn records(&self, kind: Kind, seat: &str) -> Vec<(String, Message)> {
        let dir = self.records_dir(kind, seat);
        let mut out: Vec<(String, Message)> = list_names(&dir)
            .into_iter()
            .filter_map(|id| {
                let text = fs::read_to_string(dir.join(&id)).ok()?;
                Some((id, Message::parse(&text)))
            })
            .collect();
        out.reverse();
        out
    }
}
