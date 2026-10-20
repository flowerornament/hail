//! `hail gc`: read mail leaves the live mailboxes for a monthly archive, so
//! the directories the hooks scan stay small. `Store::find` still looks in
//! the archive, so `sent` and `show` keep working on old ids.

use std::fs;
use std::io::Write;

use super::{Store, ids};
use crate::error::{Error, Result};
use crate::time;

/// Move read mail older than `days` to `archive/<yyyy-mm>/<mailbox>/` and
/// note each id in `archive/index`. Returns how many moved.
pub fn archive(store: &Store, days: u64) -> Result<usize> {
    let cutoff = time::now().as_second() - i64::try_from(days).unwrap_or(i64::MAX / 86400) * 86400;
    let index_path = store.archive_dir().join("index");
    let mut index = None;
    let mut moved = 0;
    for addr in store.mailboxes() {
        for (id, r) in store.mailbox(&addr).claimed() {
            if r.at.as_second() >= cutoff {
                continue;
            }
            let month = r.at.strftime("%Y-%m").to_string();
            let dest_dir = store
                .archive_dir()
                .join(&month)
                .join(addr.to_string())
                .join("cur");
            fs::create_dir_all(&dest_dir).map_err(Error::at(&dest_dir))?;
            // The index line first: a crash after it leaves the message
            // findable in either place, never in neither.
            if index.is_none() {
                let f = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&index_path);
                index = Some(f.map_err(Error::at(&index_path))?);
            }
            if let Some(f) = index.as_mut() {
                writeln!(f, "{id} {month} {addr}").map_err(Error::at(&index_path))?;
            }
            let file = format!("{id}.{}.md", r.how.as_str());
            fs::rename(
                store.seat_dir(&addr).join("cur").join(&file),
                dest_dir.join(&file),
            )
            .map_err(Error::at(&dest_dir))?;
            ids::forget(store, &id);
            moved += 1;
        }
    }
    Ok(moved)
}

/// What [`archive_gone`] moved.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gone {
    pub mailboxes: usize,
    pub unread: usize,
}

/// Move the mailboxes of seats whose workspace is gone, and that no hook
/// has read within `days`, to `archive/gone-<epoch>/<mailbox>/`. Each
/// message is noted in `archive/index`, so `sent` and `show` still find
/// it; its unread mail would never have been read. The seat's name is free
/// again once its mailbox has moved.
pub fn archive_gone(store: &Store, days: u64) -> Result<Gone> {
    let now = time::now();
    let quiet_for = std::time::Duration::from_secs(days.saturating_mul(86_400));
    let batch = format!("gone-{}", now.as_second());
    let index_path = store.archive_dir().join("index");
    let mut index = None;
    let mut gone = Gone::default();
    for (addr, _) in store.gone_mailboxes() {
        let mailbox = store.mailbox(&addr);
        if mailbox.hooked_within(quiet_for) {
            continue;
        }
        let unread = mailbox.unread();
        let ids: Vec<_> = unread
            .iter()
            .cloned()
            .chain(mailbox.claimed().into_iter().map(|(id, _)| id))
            .collect();
        let dest = store.archive_dir().join(&batch).join(addr.to_string());
        let parent = store.archive_dir().join(&batch);
        fs::create_dir_all(&parent).map_err(Error::at(&parent))?;
        // As in `archive`: the index lines first, so a crash leaves each
        // message findable in one place or the other.
        if !ids.is_empty() {
            if index.is_none() {
                let f = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&index_path);
                index = Some(f.map_err(Error::at(&index_path))?);
            }
            if let Some(f) = index.as_mut() {
                for id in &ids {
                    writeln!(f, "{id} {batch} {addr}").map_err(Error::at(&index_path))?;
                }
            }
        }
        fs::rename(store.seat_dir(&addr), &dest).map_err(Error::at(&dest))?;
        for id in &ids {
            ids::forget(store, id);
        }
        gone.mailboxes += 1;
        gone.unread += unread.len();
    }
    Ok(gone)
}
