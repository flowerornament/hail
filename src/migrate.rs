//! The one-time import of 0.3 state (spec §11.1), its revert, and `gc`.
//! The transition lives here so the rest of the code reads as if the bash
//! never existed; 0.5 deletes the import and revert.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::ctx::{Ctx, PaneMap, sub_seat_eligible};
use crate::error::{Error, Result};
use crate::seat;
use crate::store::mailbox::{How, Receipt};
use crate::store::message::Message;
use crate::store::records::Kind;
use crate::store::{Store, ids, list_names, write_atomic};
use crate::time;
use crate::transport::tmux::Tmux;

const LEGACY_DIRS: [&str; 6] = [
    "inbox",
    "obligations",
    "sent",
    "identity",
    "incarnation",
    "read",
];

struct Lock(PathBuf);

impl Lock {
    /// `.migrate.lock` holds a pid and a start time; a lock whose pid is gone
    /// is taken over. The content is written before the lock appears (a
    /// hard link of a finished file fails if the name exists), so a second
    /// taker never reads an empty lock and mistakes it for a stale one.
    fn take(store: &Store) -> Result<Lock> {
        let path = store.root().join(".migrate.lock");
        fs::create_dir_all(store.root()).map_err(|e| Error::io(store.root(), e))?;
        let mine = store
            .root()
            .join(format!(".migrate.lock.{}", std::process::id()));
        fs::write(
            &mine,
            format!("{} {}\n", std::process::id(), time::iso(time::now())),
        )
        .map_err(|e| Error::io(&mine, e))?;
        let result = (|| {
            for _ in 0..2 {
                match fs::hard_link(&mine, &path) {
                    Ok(()) => return Ok(Lock(path.clone())),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        let pid: u32 = fs::read_to_string(&path)
                            .ok()
                            .and_then(|s| s.split(' ').next()?.parse().ok())
                            .unwrap_or(0);
                        if pid != 0 && alive(pid) {
                            return Err(Error::State(format!(
                                "a migration is running (pid {pid}); wait for it"
                            )));
                        }
                        let _ = fs::remove_file(&path);
                    }
                    Err(e) => return Err(Error::io(&path, e)),
                }
            }
            Err(Error::State(format!("cannot take {}", path.display())))
        })();
        let _ = fs::remove_file(&mine);
        result
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}

fn legacy_receipt(read_file: &Path) -> Option<Receipt> {
    let c = fs::read_to_string(read_file).ok()?;
    let c = c.trim();
    let (how, t) = match c.split_once(' ') {
        Some(("injected", t)) => (How::Injected, t),
        Some(("inline", t)) => (How::Inline, t),
        _ => (How::Read, c),
    };
    Some(Receipt {
        how,
        at: time::parse_iso(t).unwrap_or_else(time::now),
    })
}

pub fn migrate(ctx: &Ctx) -> Result<u8> {
    let store = &ctx.store;
    let _lock = Lock::take(store)?;
    if !store.legacy_present() {
        outln!(
            "nothing to migrate: no 0.3 state in {}",
            store.root().display()
        );
        return Ok(0);
    }
    let pm = Tmux::detect()
        .ok()
        .and_then(|t| PaneMap::load(&t, ctx.home.as_deref()).ok());
    // 0.3 keyed inboxes by label (now a seat name) or, unlabeled, by pane id.
    // Recent unread mail under a live pane id maps to that pane's seat (or
    // sub-seat) so the agent there reads it; everything else under a pane id
    // is kept apart as legacy-%N, since the pane may hold another agent now.
    let map_key = |key: &str| -> String {
        if !key.starts_with('%') {
            return key.to_string();
        }
        let Some(pm) = &pm else {
            return format!("legacy-{key}");
        };
        let Some(p) = pm.find(key) else {
            return format!("legacy-{key}");
        };
        let Some(seat) = pm.seat_of(p) else {
            return format!("legacy-{key}");
        };
        if pm.agents_in(seat).len() > 1 && sub_seat_eligible(p) {
            seat::sub_seat(seat, key)
        } else {
            seat.to_string()
        }
    };

    let legacy_key = |key: &str| -> String {
        if key.starts_with('%') {
            format!("legacy-{key}")
        } else {
            key.to_string()
        }
    };
    let root = store.root().to_path_buf();
    let (mut unread, mut read, mut owed, mut pending) = (0, 0, 0, 0);
    let recent_cutoff = time::now().as_second() - 2 * 24 * 3600;
    let mut unread_by: std::collections::BTreeMap<String, usize> = Default::default();
    for key in list_names(&root.join("inbox")) {
        let dir = root.join("inbox").join(&key);
        for name in list_names(&dir) {
            let Some(id) = name.strip_suffix(".md") else {
                continue;
            };
            let receipt = legacy_receipt(&dir.join(format!("{id}.read")));
            let recent = fs::metadata(dir.join(&name))
                .and_then(|m| m.modified())
                .is_ok_and(|t| time::from_system(t).as_second() >= recent_cutoff);
            let dest = if receipt.is_none() && recent {
                map_key(&key)
            } else {
                legacy_key(&key)
            };
            store.mailbox(&dest).import(id, &dir.join(&name), receipt)?;
            ids::index(store, id, &dest)?;
            if receipt.is_some() {
                read += 1
            } else {
                unread += 1;
                *unread_by.entry(dest).or_insert(0) += 1;
            }
        }
    }
    for key in list_names(&root.join("obligations")) {
        let dir = root.join("obligations").join(&key);
        // Obligations keyed by a pane id are artefacts of the 0.3 identity bug
        // (an unlabeled pane, or every Codex seat as %2): parked, not dumped
        // on whichever seat sits in that pane today.
        let dest = store.seat_dir(&legacy_key(&key)).join("owed");
        fs::create_dir_all(&dest).map_err(|e| Error::io(&dest, e))?;
        for id in list_names(&dir) {
            fs::rename(dir.join(&id), dest.join(&id)).map_err(|e| Error::io(&dest, e))?;
            owed += 1;
        }
    }
    let week_ago = time::now().as_second() - 7 * 24 * 3600;
    for key in list_names(&root.join("sent")) {
        let dir = root.join("sent").join(&key);
        let me = legacy_key(&key);
        for id in list_names(&dir) {
            let Ok(text) = fs::read_to_string(dir.join(&id)) else {
                continue;
            };
            let rec = Message::parse(&text);
            let epoch: i64 = rec.get("epoch").and_then(|e| e.parse().ok()).unwrap_or(0);
            let received = ids::lookup(store, &id)
                .and_then(|s| store.mailbox(&s).find(&id))
                .is_some_and(|f| f.receipt.is_some());
            if epoch >= week_ago && !received {
                store.put_record(Kind::Pending, &me, &id, &rec)?;
                pending += 1;
            }
        }
    }
    let archive = store.archive_dir().join("0.3");
    fs::create_dir_all(&archive).map_err(|e| Error::io(&archive, e))?;
    for d in LEGACY_DIRS {
        let from = root.join(d);
        if from.exists() {
            fs::rename(&from, archive.join(d)).map_err(|e| Error::io(&from, e))?;
        }
    }
    write_atomic(
        &archive.join("MIGRATED"),
        format!(
            "{} unread={unread} read={read} owed={owed} pending={pending}\n",
            time::iso(time::now())
        )
        .as_bytes(),
    )?;
    outln!(
        "migrated 0.3 state: {unread} unread and {read} read messages, {owed} obligations, {pending} pending sends; holds kept; the old tree is in {}",
        archive.display()
    );
    // Unread mail where no agent sits now (an old label, a dead pane) is not
    // delivered by anyone: say where it is.
    let stranded: Vec<String> = unread_by
        .iter()
        .filter(|(seat, _)| {
            let base = seat::split_sub_seat(seat).0;
            pm.as_ref().is_none_or(|pm| pm.in_seat(base).is_empty())
        })
        .map(|(seat, n)| format!("  {seat}: {n} unread"))
        .collect();
    if !stranded.is_empty() {
        outln!(
            "no agent sits in these seats now, so their unread mail waits (hail seats lists them; hail show <id> reads one; hail doctor keeps reporting them):\n{}",
            stranded.join("\n")
        );
    }
    Ok(0)
}

/// Put the 0.3 tree back and export everything 0.4 holds into it, so the
/// bash version reads mail sent since the cutover.
pub fn revert(ctx: &Ctx) -> Result<u8> {
    let store = &ctx.store;
    let _lock = Lock::take(store)?;
    let root = store.root().to_path_buf();
    let archive = store.archive_dir().join("0.3");
    if !archive.is_dir() {
        return Err(Error::State(format!(
            "nothing to revert: {} is missing",
            archive.display()
        )));
    }
    for d in LEGACY_DIRS {
        let (from, to) = (archive.join(d), root.join(d));
        if from.exists() && !to.exists() {
            fs::rename(&from, &to).map_err(|e| Error::io(&from, e))?;
        }
    }
    let mut n = 0;
    for name in store.seat_names() {
        // 0.3 keys: a label, or a pane id for legacy-%N and <seat>@%N.
        let key = match (name.strip_prefix("legacy-"), seat::split_sub_seat(&name)) {
            (Some(pane), _) => pane.to_string(),
            (None, (_, Some(pane))) => pane.to_string(),
            (None, (base, None)) => base.to_string(),
        };
        let inbox = root.join("inbox").join(&key);
        fs::create_dir_all(&inbox).map_err(|e| Error::io(&inbox, e))?;
        let seat_dir = store.seat_dir(&name);
        for id in store.mailbox(&name).unread() {
            copy(
                &seat_dir.join("new").join(format!("{id}.md")),
                &inbox.join(format!("{id}.md")),
            )?;
            n += 1;
        }
        for (id, r) in store.mailbox(&name).claimed() {
            copy(
                &seat_dir
                    .join("cur")
                    .join(format!("{id}.{}.md", r.how.as_str())),
                &inbox.join(format!("{id}.md")),
            )?;
            let mark = match r.how {
                How::Read => time::iso(r.at),
                how => format!("{} {}", how.as_str(), time::iso(r.at)),
            };
            fs::write(inbox.join(format!("{id}.read")), format!("{mark}\n"))
                .map_err(|e| Error::io(&inbox, e))?;
            n += 1;
        }
        for (sub, legacy) in [("owed", "obligations"), ("pending", "sent")] {
            let dest = root.join(legacy).join(&key);
            for id in list_names(&seat_dir.join(sub)) {
                fs::create_dir_all(&dest).map_err(|e| Error::io(&dest, e))?;
                copy(&seat_dir.join(sub).join(&id), &dest.join(&id))?;
            }
        }
    }
    let parked = store
        .archive_dir()
        .join(format!("0.4-reverted-{}", time::id_stamp(time::now())));
    fs::create_dir_all(&parked).map_err(|e| Error::io(&parked, e))?;
    for d in ["seats", "ids"] {
        let from = root.join(d);
        if from.exists() {
            fs::rename(&from, parked.join(d)).map_err(|e| Error::io(&from, e))?;
        }
    }
    outln!(
        "reverted to the 0.3 layout: {n} messages exported; 0.4 state parked in {}",
        parked.display()
    );
    Ok(0)
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        return Ok(());
    }
    fs::copy(from, to).map(|_| ()).map_err(|e| Error::io(to, e))
}

/// Move read mail older than `days` to `archive/<yyyy-mm>/<seat>/` and note
/// each id in `archive/index`, so `sent` and `show` still find it.
pub fn gc(ctx: &Ctx, days: u64) -> Result<u8> {
    use std::io::Write;
    let store = &ctx.store;
    let cutoff = time::now().as_second() - (days as i64) * 86400;
    let mut moved = 0;
    let index_path = store.archive_dir().join("index");
    let mut index = None;
    for name in store.seat_names() {
        let mb = store.mailbox(&name);
        for (id, r) in mb.claimed() {
            if r.at.as_second() >= cutoff {
                continue;
            }
            let month = r.at.strftime("%Y-%m").to_string();
            let dest_dir = store.archive_dir().join(&month).join(&name).join("cur");
            fs::create_dir_all(&dest_dir).map_err(|e| Error::io(&dest_dir, e))?;
            // The index line first: a crash after it leaves the message
            // findable in either place, never in neither.
            if index.is_none() {
                index = Some(
                    fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&index_path)
                        .map_err(|e| Error::io(&index_path, e))?,
                );
            }
            if let Some(f) = index.as_mut() {
                writeln!(f, "{id} {month} {name}").map_err(|e| Error::io(&index_path, e))?;
            }
            let file = format!("{id}.{}.md", r.how.as_str());
            fs::rename(
                store.seat_dir(&name).join("cur").join(&file),
                dest_dir.join(&file),
            )
            .map_err(|e| Error::io(&dest_dir, e))?;
            ids::forget(store, &id);
            moved += 1;
        }
    }
    outln!(
        "archived {moved} read messages older than {days} days into {}",
        store.archive_dir().display()
    );
    Ok(0)
}
