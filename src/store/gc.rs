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
