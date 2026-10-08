//! Message ids and the id index: `ids/<id>` is a symlink whose target is the
//! mailbox name. Creating a symlink fails if the name exists, so reserving
//! an id is atomic and ids stay unique under any concurrency; lookups are one
//! readlink.

use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;

use jiff::Timestamp;

use super::Store;
use crate::error::{Error, Result};
use crate::seat::Addr;
use crate::time;

/// A message id: `MMDDTHHMMSS-xxxx` for new messages, whatever 0.3 wrote
/// for old ones. Valid as one path component, so it can never escape the
/// directory it names a file in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

impl Id {
    pub fn parse(s: &str) -> Option<Self> {
        let ok = !s.is_empty() && s != "." && s != ".." && !s.contains('/') && !s.contains('\0');
        ok.then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Four hex digits. getrandom fails only when the OS has no entropy source
/// at all; the time is a fallback, and the index still keeps ids unique.
fn random_suffix() -> String {
    let mut b = [0u8; 2];
    if getrandom::fill(&mut b).is_err() {
        b = time::now().subsec_nanosecond().to_le_bytes()[..2]
            .try_into()
            .unwrap_or_default();
    }
    format!("{:04x}", u16::from_le_bytes(b))
}

/// Reserve a fresh id for a message to `to`.
pub fn reserve(store: &Store, to: &Addr, at: Timestamp) -> Result<Id> {
    let dir = store.ids_dir();
    fs::create_dir_all(&dir).map_err(Error::at(&dir))?;
    let stamp = time::id_stamp(at);
    for _ in 0..256 {
        let id = Id(format!("{stamp}-{}", random_suffix()));
        match symlink(to.to_string(), dir.join(id.as_str())) {
            Ok(()) => return Ok(id),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(Error::at(&dir.join(id.as_str()))(e)),
        }
    }
    Err(Error::State(format!(
        "could not reserve a message id in {}",
        dir.display()
    )))
}

/// Index an existing id (migration). An entry already present is left alone.
pub fn index(store: &Store, id: &Id, to: &Addr) -> Result<()> {
    let dir = store.ids_dir();
    fs::create_dir_all(&dir).map_err(Error::at(&dir))?;
    match symlink(to.to_string(), dir.join(id.as_str())) {
        Err(e) if e.kind() != ErrorKind::AlreadyExists => Err(Error::at(&dir.join(id.as_str()))(e)),
        _ => Ok(()),
    }
}

/// The mailbox a message id was sent to.
pub fn lookup(store: &Store, id: &Id) -> Option<Addr> {
    let target = fs::read_link(store.ids_dir().join(id.as_str())).ok()?;
    Addr::parse(&target.to_string_lossy())
}

pub fn forget(store: &Store, id: &Id) {
    let _ = fs::remove_file(store.ids_dir().join(id.as_str()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn ids_are_one_path_component() {
        assert!(Id::parse("1006T190046-649b").is_some());
        for bad in ["", ".", "..", "a/b", "x\0"] {
            assert!(Id::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn parallel_reservations_never_collide() {
        let t = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::at(t.path()));
        let at = time::now();
        let to = Addr::parse("w").unwrap();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (store, to) = (store.clone(), to.clone());
                std::thread::spawn(move || {
                    (0..125)
                        .map(|_| reserve(&store, &to, at).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let ids: Vec<Id> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 1000);
        assert_eq!(lookup(&store, &ids[0]), Some(to));
    }
}
