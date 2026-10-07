//! Standing state as small header-only records, typed here so the header
//! names live in one place:
//! - `seats/<mailbox>/owed/<id>`: obligations on a mailbox ([`Entry`]);
//! - `holds/<id>`: holds and blocks in effect, global ([`Entry`]);
//! - `seats/<mailbox>/pending/<id>`: my sends with no receipt yet ([`Pending`]).

use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use super::ids::Id;
use super::message::Message;
use super::{Store, list_names, write_atomic};
use crate::envelope::Kind;
use crate::error::{Error, Result};
use crate::seat::Addr;
use crate::time;

/// An obligation or a hold: what was asked, by whom, of whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: Id,
    /// `None` when a 0.3 record names a kind 0.4 does not know.
    pub kind: Option<Kind>,
    pub issuer: Addr,
    pub to: Addr,
    pub scope: Option<String>,
    pub time: Option<Timestamp>,
    pub re: Option<Id>,
    pub headline: String,
}

/// A send not yet received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: Id,
    pub kind: Option<Kind>,
    pub to: Addr,
    pub sent: Option<Timestamp>,
    pub headline: String,
}

impl Entry {
    fn to_message(&self) -> Message {
        Message::default()
            .header("id", self.id.as_str())
            .header("kind", self.kind.map_or("?", Kind::as_str))
            .header("issuer", self.issuer.to_string())
            .header("to", self.to.to_string())
            .header("scope", self.scope.as_deref().unwrap_or(""))
            .header("time", self.time.map(time::iso).unwrap_or_default())
            .header_opt("re", self.re.as_ref().map(Id::as_str))
            .header("headline", &self.headline)
    }

    /// Lenient: a 0.3 record may lack fields; the file name is the id.
    fn from_message(id: Id, m: &Message) -> Self {
        Self {
            id,
            kind: m.get("kind").and_then(Kind::parse),
            issuer: Addr::parse(m.get("issuer").unwrap_or("?")),
            to: Addr::parse(m.get("to").unwrap_or("?")),
            scope: m.get("scope").filter(|s| !s.is_empty()).map(str::to_string),
            time: m.get("time").and_then(time::parse_iso),
            re: m.get("re").and_then(Id::parse),
            headline: m.get("headline").unwrap_or("").to_string(),
        }
    }
}

impl Pending {
    fn to_message(&self) -> Message {
        Message::default()
            .header("id", self.id.as_str())
            .header("kind", self.kind.map_or("?", Kind::as_str))
            .header("to", self.to.to_string())
            .header(
                "epoch",
                self.sent.map_or(0, Timestamp::as_second).to_string(),
            )
            .header("headline", &self.headline)
    }

    /// 0.3 wrote the send time as epoch seconds; so does 0.4, for revert.
    pub fn from_message(id: Id, m: &Message) -> Self {
        Self {
            id,
            kind: m.get("kind").and_then(Kind::parse),
            to: Addr::parse(m.get("to").unwrap_or("?")),
            sent: m
                .get("epoch")
                .and_then(|e| e.parse().ok())
                .and_then(|s| Timestamp::from_second(s).ok()),
            headline: m.get("headline").unwrap_or("").to_string(),
        }
    }
}

impl Store {
    fn owed_dir(&self, to: &Addr) -> PathBuf {
        self.seat_dir(to).join("owed")
    }

    fn pending_dir(&self, from: &Addr) -> PathBuf {
        self.seat_dir(from).join("pending")
    }

    pub fn owed(&self, to: &Addr) -> Vec<Entry> {
        read_all(&self.owed_dir(to))
            .map(|(id, m)| Entry::from_message(id, &m))
            .collect()
    }

    pub fn has_owed(&self, to: &Addr, id: &Id) -> bool {
        self.owed_dir(to).join(id.as_str()).is_file()
    }

    pub fn put_owed(&self, e: &Entry) -> Result<()> {
        write_atomic(
            &self.owed_dir(&e.to).join(e.id.as_str()),
            e.to_message().render_record().as_bytes(),
        )
    }

    pub fn remove_owed(&self, to: &Addr, id: &Id) -> Result<()> {
        remove(&self.owed_dir(to).join(id.as_str()))
    }

    /// Holds and blocks in effect, newest first.
    pub fn holds(&self) -> Vec<Entry> {
        read_all(&self.holds_dir())
            .map(|(id, m)| Entry::from_message(id, &m))
            .collect()
    }

    pub fn has_hold(&self, id: &Id) -> bool {
        self.holds_dir().join(id.as_str()).is_file()
    }

    pub fn put_hold(&self, e: &Entry) -> Result<()> {
        write_atomic(
            &self.holds_dir().join(e.id.as_str()),
            e.to_message().render_record().as_bytes(),
        )
    }

    pub fn remove_hold(&self, id: &Id) -> Result<()> {
        remove(&self.holds_dir().join(id.as_str()))
    }

    pub fn pending(&self, from: &Addr) -> Vec<Pending> {
        read_all(&self.pending_dir(from))
            .map(|(id, m)| Pending::from_message(id, &m))
            .collect()
    }

    pub fn put_pending(&self, from: &Addr, p: &Pending) -> Result<()> {
        write_atomic(
            &self.pending_dir(from).join(p.id.as_str()),
            p.to_message().render_record().as_bytes(),
        )
    }

    pub fn remove_pending(&self, from: &Addr, id: &Id) -> Result<()> {
        remove(&self.pending_dir(from).join(id.as_str()))
    }
}

/// Records in a directory, newest first (ids sort by time).
fn read_all(dir: &Path) -> impl Iterator<Item = (Id, Message)> + use<> {
    let dir = dir.to_path_buf();
    let mut names = list_names(&dir);
    names.reverse();
    names.into_iter().filter_map(move |name| {
        let text = fs::read_to_string(dir.join(&name)).ok()?;
        Some((Id::parse(&name)?, Message::parse(&text)))
    })
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::at(path)(e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::at(t.path());
        let e = Entry {
            id: Id::parse("1007T000000-aaaa").unwrap(),
            kind: Some(Kind::Go),
            issuer: Addr::parse("boss"),
            to: Addr::parse("hail@%28"),
            scope: Some("commit".into()),
            time: Some(time::now().round(jiff::Unit::Second).unwrap()),
            re: None,
            headline: "GO".into(),
        };
        store.put_owed(&e).unwrap();
        assert_eq!(store.owed(&e.to), vec![e.clone()]);
        assert!(store.has_owed(&e.to, &e.id));
        store.remove_owed(&e.to, &e.id).unwrap();
        assert_eq!(store.owed(&e.to), vec![]);
        store.put_hold(&e).unwrap();
        assert_eq!(store.holds(), vec![e]);
    }
}
