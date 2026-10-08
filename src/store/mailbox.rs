//! A seat's Maildir: `tmp/` while writing, `new/` unread, `cur/` claimed.
//! The claim is `rename(new/<id>.md, cur/<id>.<how>.md)`: exactly one claimer
//! wins, so a body is never injected twice, and the receipt
//! cannot exist before the claim. The claimed file's mtime is the receipt time.

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;

use super::ids::Id;
use crate::error::{Error, Result};
use crate::time;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// A hook put the body in the agent's context.
    Injected,
    /// The agent ran `hail inbox`.
    Read,
    /// A control kind: complete in the envelope, claimed at send.
    Inline,
}

impl How {
    pub const ALL: [Self; 3] = [Self::Injected, Self::Read, Self::Inline];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Injected => "injected",
            Self::Read => "read",
            Self::Inline => "inline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    pub how: How,
    pub at: Timestamp,
}

impl std::fmt::Display for Receipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.how.as_str(), time::iso(self.at))
    }
}

/// Where a message is and whether it has been claimed.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub receipt: Option<Receipt>,
}

pub struct Mailbox {
    dir: PathBuf,
}

impl Mailbox {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn sub(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn new_path(&self, id: &Id) -> PathBuf {
        self.sub("new").join(format!("{id}.md"))
    }

    fn cur_path(&self, id: &Id, how: How) -> PathBuf {
        self.sub("cur").join(format!("{id}.{}.md", how.as_str()))
    }

    /// Note that this mailbox's prompt hook ran: `deliver` calls it every
    /// time, so a send can tell a seat with working hooks from a quiet one.
    pub fn touch_hooked(&self) {
        let p = self.sub("hooked");
        if File::options().write(true).open(&p).is_err() {
            let _ = fs::create_dir_all(&self.dir);
            let _ = File::create(&p);
        }
        set_mtime(&p, time::now());
    }

    /// When a prompt hook last read this mailbox, if ever.
    pub fn hooked_at(&self) -> Option<Timestamp> {
        mtime(&self.sub("hooked"))
    }

    /// Whether the prompt hook ran within `within`.
    pub fn hooked_within(&self, within: std::time::Duration) -> bool {
        self.hooked_at()
            .is_some_and(|t| time::now().duration_since(t).unsigned_abs() < within)
    }

    /// Write a message durably, then make it visible: `new/` for mail,
    /// `cur/<id>.inline.md` for a control kind.
    pub fn post(&self, id: &Id, text: &str, control: bool) -> Result<PathBuf> {
        let tmp = self.sub("tmp").join(format!("{id}.md"));
        let dest = if control {
            self.cur_path(id, How::Inline)
        } else {
            self.new_path(id)
        };
        for d in [self.sub("tmp"), self.sub("new"), self.sub("cur")] {
            fs::create_dir_all(&d).map_err(Error::at(&d))?;
        }
        let mut f = File::create(&tmp).map_err(Error::at(&tmp))?;
        f.write_all(text.as_bytes()).map_err(Error::at(&tmp))?;
        f.sync_all().map_err(Error::at(&tmp))?;
        fs::rename(&tmp, &dest).map_err(Error::at(&dest))?;
        Ok(dest)
    }

    /// Place an existing file (migration) as unread or claimed at a time.
    pub fn import(&self, id: &Id, from: &Path, receipt: Option<Receipt>) -> Result<()> {
        let dest = match receipt {
            None => self.new_path(id),
            Some(r) => self.cur_path(id, r.how),
        };
        let dir = dest.parent().unwrap_or(&self.dir).to_path_buf();
        fs::create_dir_all(&dir).map_err(Error::at(&dir))?;
        fs::rename(from, &dest).map_err(Error::at(&dest))?;
        if let Some(r) = receipt {
            set_mtime(&dest, r.at);
        }
        Ok(())
    }

    /// Unread ids, oldest first. Ids sort by time only to the second, and two
    /// sends in one second would then sort by their random suffix; the
    /// file's arrival time (nanoseconds) orders them as they were sent.
    pub fn unread(&self) -> Vec<Id> {
        let dir = self.sub("new");
        let mut ids: Vec<(Option<SystemTime>, Id)> = super::list_names(&dir)
            .into_iter()
            .filter_map(|n| {
                let id = Id::parse(n.strip_suffix(".md")?)?;
                let at = fs::metadata(dir.join(&n)).and_then(|m| m.modified()).ok();
                Some((at, id))
            })
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }

    /// Claimed ids with their receipts, oldest first.
    pub fn claimed(&self) -> Vec<(Id, Receipt)> {
        super::list_names(&self.sub("cur"))
            .into_iter()
            .filter_map(|n| {
                let stem = n.strip_suffix(".md")?;
                let (id, how) = stem.rsplit_once('.')?;
                let how = How::ALL.into_iter().find(|h| h.as_str() == how)?;
                let id = Id::parse(id)?;
                let at = mtime(&self.cur_path(&id, how))?;
                Some((id, Receipt { how, at }))
            })
            .collect()
    }

    /// Claim one unread message. `None` when another claimer won the race.
    pub fn claim(&self, id: &Id, how: How) -> Result<Option<PathBuf>> {
        let dest = self.cur_path(id, how);
        let cur = self.sub("cur");
        fs::create_dir_all(&cur).map_err(Error::at(&cur))?;
        match fs::rename(self.new_path(id), &dest) {
            Ok(()) => {
                // A crash before this leaves the send time as the receipt
                // time: earlier, never later. Harmless.
                set_mtime(&dest, time::now());
                Ok(Some(dest))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::at(&dest)(e)),
        }
    }

    /// Undo a claim whose output never reached the agent: at least once.
    pub fn unclaim(&self, id: &Id, how: How) {
        let _ = fs::rename(self.cur_path(id, how), self.new_path(id));
    }

    /// At most four stats: the three claimed names, then unread.
    pub fn find(&self, id: &Id) -> Option<Found> {
        for how in How::ALL {
            let p = self.cur_path(id, how);
            if let Some(at) = mtime(&p) {
                return Some(Found {
                    path: p,
                    receipt: Some(Receipt { how, at }),
                });
            }
        }
        let p = self.new_path(id);
        p.is_file().then_some(Found {
            path: p,
            receipt: None,
        })
    }
}

fn mtime(p: &Path) -> Option<Timestamp> {
    fs::metadata(p).ok()?.modified().ok().map(time::from_system)
}

pub fn set_mtime(p: &Path, at: Timestamp) {
    if let Ok(f) = File::options().write(true).open(p) {
        let _ = f.set_modified(SystemTime::from(at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn id(s: &str) -> Id {
        Id::parse(s).unwrap()
    }

    #[test]
    fn exactly_one_claimer_wins() {
        let t = tempfile::tempdir().unwrap();
        let mb = Arc::new(Mailbox::new(t.path().join("w")));
        for i in 0..50 {
            mb.post(&id(&format!("id{i:02}")), "kind: fyi\n\nx\n", false)
                .unwrap();
        }
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let mb = mb.clone();
                std::thread::spawn(move || {
                    mb.unread()
                        .iter()
                        .filter(|id| mb.claim(id, How::Injected).unwrap().is_some())
                        .count()
                })
            })
            .collect();
        let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(total, 50);
        assert_eq!(mb.unread(), Vec::<Id>::new());
        assert_eq!(mb.claimed().len(), 50);
    }

    #[test]
    fn unclaim_redelivers_under_the_same_id() {
        let t = tempfile::tempdir().unwrap();
        let mb = Mailbox::new(t.path().join("w"));
        mb.post(&id("a"), "x\n", false).unwrap();
        mb.claim(&id("a"), How::Injected).unwrap();
        assert!(mb.find(&id("a")).unwrap().receipt.is_some());
        mb.unclaim(&id("a"), How::Injected);
        assert_eq!(mb.unread(), vec![id("a")]);
    }

    #[test]
    fn control_kinds_are_claimed_at_post() {
        let t = tempfile::tempdir().unwrap();
        let mb = Mailbox::new(t.path().join("w"));
        mb.post(&id("s"), "x\n", true).unwrap();
        assert_eq!(mb.unread(), Vec::<Id>::new());
        assert_eq!(mb.find(&id("s")).unwrap().receipt.unwrap().how, How::Inline);
    }
}
