//! The one-time import of 0.3 state (spec §11.1), and its revert.
//! The transition lives here so the rest of the code reads as if the bash
//! never existed; 0.5 deletes the import and revert.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::policy::{MIGRATE_RECENT, PENDING_LAPSE, secs};
use crate::route::can_hold_sub_seat;
use crate::seat::Addr;
use crate::store::ids::{self, Id};
use crate::store::mailbox::{How, Receipt};
use crate::store::message::Message;
use crate::store::records::Pending;
use crate::store::{Status, Store, list_names, write_atomic};
use crate::time;
use crate::transport::pane_map::PaneMap;
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
    fn take(store: &Store) -> Result<Self> {
        let path = store.root().join(".migrate.lock");
        fs::create_dir_all(store.root()).map_err(Error::at(store.root()))?;
        let mine = store
            .root()
            .join(format!(".migrate.lock.{}", std::process::id()));
        fs::write(
            &mine,
            format!("{} {}\n", std::process::id(), time::iso(time::now())),
        )
        .map_err(Error::at(&mine))?;
        let result = (|| {
            for _ in 0..2 {
                match fs::hard_link(&mine, &path) {
                    Ok(()) => return Ok(Self(path.clone())),
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
                    Err(e) => return Err(Error::at(&path)(e)),
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

/// Whether a process exists. `kill -0` rather than a signal call: the crate
/// forbids `unsafe`, and std has no portable way to ask.
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

/// Where 0.3 state goes. 0.3 keyed by label (now a seat name) or, for an
/// unlabeled pane, by pane id.
struct Keys {
    panes: Option<PaneMap>,
}

impl Keys {
    /// Recent unread mail under a live pane id goes to that pane's seat (or
    /// sub-seat), where the agent there reads it.
    fn live(&self, key: &str) -> Addr {
        if !key.starts_with('%') {
            return Addr::parse(key);
        }
        let seat_of_pane = self.panes.as_ref().and_then(|pm| {
            let p = pm.find(key)?;
            let seat = pm.seat_of(p)?;
            Some(if pm.agents_in(seat).len() > 1 && can_hold_sub_seat(p) {
                Addr::sub(seat, key)
            } else {
                Addr::parse(seat)
            })
        });
        seat_of_pane.unwrap_or_else(|| Self::parked(key))
    }

    /// Everything else under a pane id is kept apart as `legacy-%N`: the
    /// pane may hold another agent now. Obligations and sends keyed so are
    /// artefacts of the 0.3 identity bug (376 on %1, 300 on %2).
    fn parked(key: &str) -> Addr {
        if key.starts_with('%') {
            Addr::Seat(format!("legacy-{key}"))
        } else {
            Addr::parse(key)
        }
    }
}

#[derive(Default)]
struct Counts {
    unread: usize,
    read: usize,
    owed: usize,
    pending: usize,
    /// Unread messages per destination mailbox, to report the stranded ones.
    unread_by: BTreeMap<Addr, usize>,
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
    let keys = Keys {
        panes: Tmux::detect()
            .ok()
            .and_then(|t| PaneMap::load(&t, ctx.home.as_deref()).ok()),
    };
    let mut counts = Counts::default();
    import_inbox(store, &keys, &mut counts)?;
    import_obligations(store, &mut counts)?;
    import_pending(store, &mut counts)?;
    let archive = park_legacy_tree(store, &counts)?;
    outln!(
        "migrated 0.3 state: {} unread and {} read messages, {} obligations, {} pending sends; holds kept; the old tree is in {}",
        counts.unread,
        counts.read,
        counts.owed,
        counts.pending,
        archive.display()
    );
    report_stranded(&keys, &counts);
    Ok(0)
}

fn import_inbox(store: &Store, keys: &Keys, counts: &mut Counts) -> Result<()> {
    let root = store.root().join("inbox");
    let recent = time::now().as_second() - secs(MIGRATE_RECENT);
    for key in list_names(&root) {
        let dir = root.join(&key);
        for name in list_names(&dir) {
            let Some(id) = name.strip_suffix(".md").and_then(Id::parse) else {
                continue;
            };
            let receipt = legacy_receipt(&dir.join(format!("{id}.read")));
            let fresh = fs::metadata(dir.join(&name))
                .and_then(|m| m.modified())
                .is_ok_and(|t| time::from_system(t).as_second() >= recent);
            let dest = if receipt.is_none() && fresh {
                keys.live(&key)
            } else {
                Keys::parked(&key)
            };
            store
                .mailbox(&dest)
                .import(&id, &dir.join(&name), receipt)?;
            ids::index(store, &id, &dest)?;
            if receipt.is_some() {
                counts.read += 1;
            } else {
                counts.unread += 1;
                *counts.unread_by.entry(dest).or_default() += 1;
            }
        }
    }
    Ok(())
}

/// Obligation files move as they are: the record format did not change.
fn import_obligations(store: &Store, counts: &mut Counts) -> Result<()> {
    let root = store.root().join("obligations");
    for key in list_names(&root) {
        let dir = root.join(&key);
        let dest = store.seat_dir(&Keys::parked(&key)).join("owed");
        fs::create_dir_all(&dest).map_err(Error::at(&dest))?;
        for id in list_names(&dir) {
            fs::rename(dir.join(&id), dest.join(&id)).map_err(Error::at(&dest))?;
            counts.owed += 1;
        }
    }
    Ok(())
}

/// Sends from the last week with no receipt stay pending; older ones lapse.
fn import_pending(store: &Store, counts: &mut Counts) -> Result<()> {
    let root = store.root().join("sent");
    let lapse = time::now().as_second() - secs(PENDING_LAPSE);
    for key in list_names(&root) {
        let dir = root.join(&key);
        let me = Keys::parked(&key);
        for name in list_names(&dir) {
            let Some(id) = Id::parse(&name) else { continue };
            let Ok(text) = fs::read_to_string(dir.join(&name)) else {
                continue;
            };
            let p = Pending::from_message(id, &Message::parse(&text));
            let recent = p.sent.is_some_and(|t| t.as_second() >= lapse);
            if recent && !matches!(store.status(&p.id), Status::Received(_)) {
                store.put_pending(&me, &p)?;
                counts.pending += 1;
            }
        }
    }
    Ok(())
}

/// Move the 0.3 directories to `archive/0.3/` and note what was imported.
fn park_legacy_tree(store: &Store, c: &Counts) -> Result<PathBuf> {
    let archive = store.archive_dir().join("0.3");
    fs::create_dir_all(&archive).map_err(Error::at(&archive))?;
    for d in LEGACY_DIRS {
        let from = store.root().join(d);
        if from.exists() {
            fs::rename(&from, archive.join(d)).map_err(Error::at(&from))?;
        }
    }
    let note = format!(
        "{} unread={} read={} owed={} pending={}\n",
        time::iso(time::now()),
        c.unread,
        c.read,
        c.owed,
        c.pending
    );
    write_atomic(&archive.join("MIGRATED"), note.as_bytes())?;
    Ok(archive)
}

/// Unread mail where no agent sits now (an old label, a dead pane) is not
/// delivered by anyone: say where it is.
fn report_stranded(keys: &Keys, counts: &Counts) {
    let stranded: Vec<String> = counts
        .unread_by
        .iter()
        .filter(|(addr, _)| {
            keys.panes
                .as_ref()
                .is_none_or(|pm| pm.in_seat(addr.seat()).is_empty())
        })
        .map(|(addr, n)| format!("  {addr}: {n} unread"))
        .collect();
    if !stranded.is_empty() {
        outln!(
            "no agent sits in these seats now, so their unread mail waits (hail seats lists them; hail show <id> reads one; hail doctor keeps reporting them):\n{}",
            stranded.join("\n")
        );
    }
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
            fs::rename(&from, &to).map_err(Error::at(&from))?;
        }
    }
    let mut n = 0;
    for addr in store.mailboxes() {
        // 0.3 keys: a label, or a pane id for legacy-%N and <seat>@%N.
        let key = match &addr {
            Addr::Sub { pane, .. } => pane.clone(),
            Addr::Seat(s) => s.strip_prefix("legacy-").unwrap_or(s).to_string(),
        };
        let inbox = root.join("inbox").join(&key);
        fs::create_dir_all(&inbox).map_err(Error::at(&inbox))?;
        let mb = store.mailbox(&addr);
        for id in mb.unread() {
            if let Some(f) = mb.find(&id) {
                copy(&f.path, &inbox.join(format!("{id}.md")))?;
                n += 1;
            }
        }
        for (id, r) in mb.claimed() {
            if let Some(f) = mb.find(&id) {
                copy(&f.path, &inbox.join(format!("{id}.md")))?;
            }
            let mark = match r.how {
                How::Read => time::iso(r.at),
                how => format!("{} {}", how.as_str(), time::iso(r.at)),
            };
            let read = inbox.join(format!("{id}.read"));
            fs::write(&read, format!("{mark}\n")).map_err(Error::at(&read))?;
            n += 1;
        }
        let seat_dir = store.seat_dir(&addr);
        for (sub, legacy) in [("owed", "obligations"), ("pending", "sent")] {
            let dest = root.join(legacy).join(&key);
            for id in list_names(&seat_dir.join(sub)) {
                fs::create_dir_all(&dest).map_err(Error::at(&dest))?;
                copy(&seat_dir.join(sub).join(&id), &dest.join(&id))?;
            }
        }
    }
    let parked = store
        .archive_dir()
        .join(format!("0.4-reverted-{}", time::id_stamp(time::now())));
    fs::create_dir_all(&parked).map_err(Error::at(&parked))?;
    for d in ["seats", "ids"] {
        let from = root.join(d);
        if from.exists() {
            fs::rename(&from, parked.join(d)).map_err(Error::at(&from))?;
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
    fs::copy(from, to).map(|_| ()).map_err(Error::at(to))
}
