//! The id index: `ids/<id>` is a symlink whose target is the seat name.
//! Creating a symlink fails if the name exists, so reserving an id is atomic
//! and ids stay unique under any concurrency; lookups are one readlink.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;

use jiff::Timestamp;

use super::Store;
use crate::error::{Error, Result};
use crate::time;

pub fn random_suffix() -> String {
    let mut b = [0u8; 2];
    // getrandom only fails when the OS has no entropy source at all.
    if getrandom::fill(&mut b).is_err() {
        let n = std::process::id() ^ time::now().subsec_nanosecond() as u32;
        b = [(n >> 8) as u8, n as u8];
    }
    format!("{:02x}{:02x}", b[0], b[1])
}

/// Reserve a fresh id for a message to `seat`.
pub fn reserve(store: &Store, seat: &str, at: Timestamp) -> Result<String> {
    let dir = store.ids_dir();
    fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let stamp = time::id_stamp(at);
    for _ in 0..256 {
        let id = format!("{stamp}-{}", random_suffix());
        match symlink(seat, dir.join(&id)) {
            Ok(()) => return Ok(id),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(&dir.join(&id), e)),
        }
    }
    Err(Error::State(format!(
        "could not reserve a message id in {}",
        dir.display()
    )))
}

/// Index an existing id (migration). An entry already present is left alone.
pub fn index(store: &Store, id: &str, seat: &str) -> Result<()> {
    let dir = store.ids_dir();
    fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    match symlink(seat, dir.join(id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(Error::io(&dir.join(id), e)),
    }
}

/// The seat a message id was sent to.
pub fn lookup(store: &Store, id: &str) -> Option<String> {
    if id.is_empty() || id.contains('/') {
        return None;
    }
    fs::read_link(store.ids_dir().join(id))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn forget(store: &Store, id: &str) {
    let _ = fs::remove_file(store.ids_dir().join(id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn parallel_reservations_never_collide() {
        let t = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::at(t.path()));
        let at = time::now();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    (0..125)
                        .map(|_| reserve(&store, "w", at).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let ids: Vec<String> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 1000);
        assert_eq!(lookup(&store, &ids[0]).as_deref(), Some("w"));
    }
}
