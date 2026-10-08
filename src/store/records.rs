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
use crate::policy::{BLOCK_DEFAULT, HOLD_DEFAULT};
use crate::seat::Addr;
use crate::time;

/// An obligation or a hold: what was asked, by whom, of whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: Id,
    /// `None` when a 0.3 record names a kind this version does not know.
    pub kind: Option<Kind>,
    pub issuer: Addr,
    pub to: Addr,
    pub scope: Option<String>,
    pub time: Option<Timestamp>,
    pub re: Option<Id>,
    /// When a hold lapses; `None` for obligations, and for holds recorded
    /// before holds lapsed (see [`Entry::lapses_at`]).
    pub expires: Option<Timestamp>,
    pub headline: String,
}

/// How long a hold of this kind lasts when `--for` does not say.
pub const fn hold_default(kind: Kind) -> std::time::Duration {
    match kind {
        Kind::Block => BLOCK_DEFAULT,
        _ => HOLD_DEFAULT,
    }
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
            .header_opt("expires", self.expires.map(time::iso).as_deref())
            .header("headline", &self.headline)
    }

    /// When this hold lapses: its `expires:`, else its kind's default after
    /// it was sent. A hold with neither has already lapsed.
    pub fn lapses_at(&self) -> Timestamp {
        self.expires
            .or_else(|| {
                self.time
                    .map(|t| time::after(t, hold_default(self.kind.unwrap_or(Kind::Hold))))
            })
            .unwrap_or(Timestamp::UNIX_EPOCH)
    }

    /// Lenient: a 0.3 record may lack fields; the file name is the id.
    fn from_message(id: Id, m: &Message) -> Self {
        Self {
            id,
            kind: m.get("kind").and_then(Kind::parse),
            issuer: header_addr(m, "issuer"),
            to: header_addr(m, "to"),
            scope: m.get("scope").filter(|s| !s.is_empty()).map(str::to_string),
            time: m.get("time").and_then(time::parse_iso),
            re: m.get("re").and_then(Id::parse),
            expires: m.get("expires").and_then(time::parse_iso),
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

    /// 0.3 wrote the send time as epoch seconds; so does this version, so
    /// that revert can hand it back.
    pub fn from_message(id: Id, m: &Message) -> Self {
        Self {
            id,
            kind: m.get("kind").and_then(Kind::parse),
            to: header_addr(m, "to"),
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

    fn lapsed_dir(&self) -> PathBuf {
        self.holds_dir().join("lapsed")
    }

    /// Every hold record not yet retired, lapsed or not, newest first.
    fn hold_records(&self) -> impl Iterator<Item = Entry> + use<> {
        read_all(&self.holds_dir()).map(|(id, m)| Entry::from_message(id, &m))
    }

    /// Holds and blocks in effect at `now`, newest first. Lapse is a filter,
    /// not a sweep: every reader agrees, and nothing is deleted here.
    pub fn holds(&self, now: Timestamp) -> Vec<Entry> {
        self.hold_records()
            .filter(|e| e.lapses_at() > now)
            .collect()
    }

    /// Move up to `limit` lapsed holds issued from `mine` to `holds/lapsed/`
    /// and return them, so the issuer hears once that each ended; the rest
    /// wait for the next call. Only the issuer's own brief calls this;
    /// anyone else's would swallow the notice.
    pub fn retire_lapsed(&self, mine: &[&Addr], now: Timestamp, limit: usize) -> Vec<Entry> {
        let lapsed: Vec<Entry> = self
            .hold_records()
            .filter(|e| e.lapses_at() <= now && mine.contains(&&e.issuer))
            .take(limit)
            .collect();
        if !lapsed.is_empty() {
            let _ = fs::create_dir_all(self.lapsed_dir());
        }
        lapsed
            .into_iter()
            .filter(|e| {
                let name = e.id.as_str();
                fs::rename(self.holds_dir().join(name), self.lapsed_dir().join(name)).is_ok()
            })
            .collect()
    }

    /// A hold by id, in effect or lapsed (retired or not); `None` if no such
    /// hold was ever recorded, or it was released.
    pub fn hold(&self, id: &Id) -> Option<Entry> {
        [self.holds_dir(), self.lapsed_dir()]
            .iter()
            .find_map(|d| fs::read_to_string(d.join(id.as_str())).ok())
            .map(|text| Entry::from_message(id.clone(), &Message::parse(&text)))
    }

    /// Delete lapsed holds nobody will hear about: retired or not, those that
    /// lapsed before `cutoff`, and unretired ones whose issuer has no mailbox
    /// (0.3 issuers such as `%2` never run `brief` again). Returns how many.
    pub fn sweep_holds(&self, now: Timestamp, cutoff: Timestamp) -> usize {
        let mut swept = 0;
        for dir in [self.holds_dir(), self.lapsed_dir()] {
            for (id, m) in read_all(&dir) {
                let e = Entry::from_message(id, &m);
                let at = e.lapses_at();
                let orphan = !self.seat_dir(&e.issuer).is_dir();
                if at <= now
                    && (at < cutoff || orphan)
                    && fs::remove_file(dir.join(e.id.as_str())).is_ok()
                {
                    swept += 1;
                }
            }
        }
        swept
    }

    pub fn put_hold(&self, e: &Entry) -> Result<()> {
        write_atomic(
            &self.holds_dir().join(e.id.as_str()),
            e.to_message().render_record().as_bytes(),
        )
    }

    /// Lift a hold, in effect or lapsed.
    pub fn remove_hold(&self, id: &Id) -> Result<()> {
        remove(&self.holds_dir().join(id.as_str()))?;
        remove(&self.lapsed_dir().join(id.as_str()))
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

/// An address header for display; `?` when absent or not a valid address.
fn header_addr(m: &Message, key: &str) -> Addr {
    m.get(key)
        .and_then(Addr::parse)
        .unwrap_or_else(|| Addr::Seat("?".into()))
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
            issuer: Addr::parse("boss").unwrap(),
            to: Addr::parse("hail@%28").unwrap(),
            scope: Some("commit".into()),
            time: Some(time::now().round(jiff::Unit::Second).unwrap()),
            re: None,
            expires: None,
            headline: "GO".into(),
        };
        store.put_owed(&e).unwrap();
        assert_eq!(store.owed(&e.to), vec![e.clone()]);
        assert!(store.has_owed(&e.to, &e.id));
        store.remove_owed(&e.to, &e.id).unwrap();
        assert_eq!(store.owed(&e.to), vec![]);
        store.put_hold(&e).unwrap();
        let now = time::now();
        assert_eq!(store.holds(now), vec![e]);
    }

    #[test]
    fn holds_lapse_for_every_reader_and_retire_for_their_issuer() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::at(t.path());
        let now: Timestamp = "2026-10-08T12:00:00Z".parse().unwrap();
        let hours = |h: i64| now - jiff::SignedDuration::from_hours(h);
        let boss = Addr::parse("boss").unwrap();
        fs::create_dir_all(store.seat_dir(&boss)).unwrap();
        let hold = |id: &str, kind, issuer: &str, time, expires| Entry {
            id: Id::parse(id).unwrap(),
            kind: Some(kind),
            issuer: Addr::parse(issuer).unwrap(),
            to: Addr::parse("worker").unwrap(),
            scope: None,
            time: Some(time),
            re: None,
            expires,
            headline: id.into(),
        };
        // Lapsed by its own expires:; in effect; an old record with no
        // expires: (8h default for a hold, 7d for a block); an orphan.
        let gone = hold(
            "1008T000000-aaaa",
            Kind::Hold,
            "boss",
            hours(3),
            Some(hours(1)),
        );
        let live = hold(
            "1008T000000-bbbb",
            Kind::Hold,
            "boss",
            hours(1),
            Some(now + jiff::SignedDuration::from_hours(1)),
        );
        let old_hold = hold("1001T000000-cccc", Kind::Hold, "boss", hours(9), None);
        let old_block = hold("1001T000000-dddd", Kind::Block, "boss", hours(9), None);
        let orphan = hold("0905T000000-eeee", Kind::Hold, "%2", hours(500), None);
        for e in [&gone, &live, &old_hold, &old_block, &orphan] {
            store.put_hold(e).unwrap();
        }
        let ids = |v: Vec<Entry>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store.holds(now)),
            vec![live.id.clone(), old_block.id.clone()]
        );

        // Someone else's brief retires nothing; the issuer's retires its own.
        let worker = Addr::parse("worker").unwrap();
        assert_eq!(store.retire_lapsed(&[&worker], now, usize::MAX), vec![]);
        // A brief retires only what it shows; the rest wait.
        let first = store.retire_lapsed(&[&boss], now, 1);
        assert_eq!(first.len(), 1);
        let mut retired = ids(store.retire_lapsed(&[&boss], now, usize::MAX));
        retired.extend(ids(first));
        retired.sort();
        assert_eq!(retired, vec![old_hold.id, gone.id.clone()]);
        assert_eq!(
            store.retire_lapsed(&[&boss], now, usize::MAX),
            vec![],
            "once"
        );
        // Retired holds are still found by id, so release can say so.
        assert!(store.hold(&gone.id).is_some());

        // gc: the orphan goes at once; retired ones after the cutoff.
        assert_eq!(store.sweep_holds(now, hours(48)), 1);
        assert!(store.hold(&orphan.id).is_none());
        assert_eq!(store.sweep_holds(now, now), 2);
        assert_eq!(ids(store.holds(now)), vec![live.id, old_block.id]);
    }
}
