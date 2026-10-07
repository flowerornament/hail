//! A seat's Maildir: `tmp/` while writing, `new/` unread, `cur/` claimed.
//! The claim is `rename(new/<id>.md, cur/<id>.<how>.md)`: exactly one claimer
//! wins, so a body is never injected twice (murail-m65jq), and the receipt
//! cannot exist before the claim. The claimed file's mtime is the receipt time.

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;

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
    pub const ALL: [How; 3] = [How::Injected, How::Read, How::Inline];

    pub fn as_str(self) -> &'static str {
        match self {
            How::Injected => "injected",
            How::Read => "read",
            How::Inline => "inline",
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
    pub fn new(dir: PathBuf) -> Mailbox {
        Mailbox { dir }
    }

    fn sub(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn new_path(&self, id: &str) -> PathBuf {
        self.sub("new").join(format!("{id}.md"))
    }

    fn cur_path(&self, id: &str, how: How) -> PathBuf {
        self.sub("cur").join(format!("{id}.{}.md", how.as_str()))
    }

    /// Write a message durably, then make it visible: `new/` for mail,
    /// `cur/<id>.inline.md` for a control kind.
    pub fn post(&self, id: &str, text: &str, control: bool) -> Result<PathBuf> {
        let tmp = self.sub("tmp").join(format!("{id}.md"));
        let dest = if control {
            self.cur_path(id, How::Inline)
        } else {
            self.new_path(id)
        };
        for d in [self.sub("tmp"), self.sub("new"), self.sub("cur")] {
            fs::create_dir_all(&d).map_err(|e| Error::io(&d, e))?;
        }
        let mut f = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
        f.write_all(text.as_bytes())
            .map_err(|e| Error::io(&tmp, e))?;
        f.sync_all().map_err(|e| Error::io(&tmp, e))?;
        fs::rename(&tmp, &dest).map_err(|e| Error::io(&dest, e))?;
        Ok(dest)
    }

    /// Place an existing file (migration) as unread or claimed at a time.
    pub fn import(&self, id: &str, from: &Path, receipt: Option<Receipt>) -> Result<()> {
        let dest = match receipt {
            None => self.new_path(id),
            Some(r) => self.cur_path(id, r.how),
        };
        let dir = dest.parent().unwrap_or(&self.dir).to_path_buf();
        fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        fs::rename(from, &dest).map_err(|e| Error::io(&dest, e))?;
        if let Some(r) = receipt {
            set_mtime(&dest, r.at);
        }
        Ok(())
    }

    /// Unread ids, oldest first (ids sort by time).
    pub fn unread(&self) -> Vec<String> {
        super::list_names(&self.sub("new"))
            .into_iter()
            .filter_map(|n| n.strip_suffix(".md").map(str::to_string))
            .collect()
    }

    /// Claimed ids with their receipts, oldest first.
    pub fn claimed(&self) -> Vec<(String, Receipt)> {
        super::list_names(&self.sub("cur"))
            .into_iter()
            .filter_map(|n| {
                let stem = n.strip_suffix(".md")?;
                let (id, how) = stem.rsplit_once('.')?;
                let how = How::ALL.into_iter().find(|h| h.as_str() == how)?;
                let at = mtime(&self.cur_path(id, how))?;
                Some((id.to_string(), Receipt { how, at }))
            })
            .collect()
    }

    /// Claim one unread message. `None` when another claimer won the race.
    pub fn claim(&self, id: &str, how: How) -> Result<Option<PathBuf>> {
        let dest = self.cur_path(id, how);
        let cur = self.sub("cur");
        fs::create_dir_all(&cur).map_err(|e| Error::io(&cur, e))?;
        match fs::rename(self.new_path(id), &dest) {
            Ok(()) => {
                // A crash before this leaves the send time as the receipt
                // time: earlier, never later. Harmless.
                set_mtime(&dest, time::now());
                Ok(Some(dest))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(&dest, e)),
        }
    }

    /// Undo a claim whose output never reached the agent: at least once.
    pub fn unclaim(&self, id: &str, how: How) {
        let _ = fs::rename(self.cur_path(id, how), self.new_path(id));
    }

    /// At most four stats: the three claimed names, then unread.
    pub fn find(&self, id: &str) -> Option<Found> {
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

    pub fn exists(&self) -> bool {
        self.dir.is_dir()
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

    #[test]
    fn exactly_one_claimer_wins() {
        let t = tempfile::tempdir().unwrap();
        let mb = Arc::new(Mailbox::new(t.path().join("w")));
        for i in 0..50 {
            mb.post(&format!("id{i:02}"), "kind: fyi\n\nx\n", false)
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
        assert!(mb.unread().is_empty());
        assert_eq!(mb.claimed().len(), 50);
    }

    #[test]
    fn unclaim_redelivers_under_the_same_id() {
        let t = tempfile::tempdir().unwrap();
        let mb = Mailbox::new(t.path().join("w"));
        mb.post("a", "x\n", false).unwrap();
        mb.claim("a", How::Injected).unwrap();
        assert!(mb.find("a").unwrap().receipt.is_some());
        mb.unclaim("a", How::Injected);
        assert_eq!(mb.unread(), vec!["a".to_string()]);
    }

    #[test]
    fn control_kinds_are_claimed_at_post() {
        let t = tempfile::tempdir().unwrap();
        let mb = Mailbox::new(t.path().join("w"));
        mb.post("s", "x\n", true).unwrap();
        assert!(mb.unread().is_empty());
        assert_eq!(mb.find("s").unwrap().receipt.unwrap().how, How::Inline);
    }
}
