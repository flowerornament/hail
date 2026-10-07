//! Receiving: `deliver` (hooks), `inbox`, `show`; receipts: `sent`, `await`.
//! None of these runs a subprocess (spec D8).

use std::fs;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::hooks;
use crate::store::mailbox::{Found, How};
use crate::store::message::Message;
use crate::store::{Store, ids};

/// At most this many bodies, or about this many bytes, reach the agent per
/// prompt; the rest stay unread for the next prompt or `hail inbox`. A
/// backlog (66 unread on one seat at migration) would otherwise be cut by
/// the harness's hook output limit after it was claimed: silent loss.
const DELIVER_BODIES: usize = 5;
const DELIVER_BYTES: usize = 8 * 1024;

/// Print this seat's unread bodies and claim them (`injected`). A hook must
/// never fail a session: this always exits 0, and an error is logged to
/// `hook-errors.log` with every claim it made given back.
pub fn deliver(ctx: &Ctx, format: Option<&str>) -> Result<u8> {
    if let Err(e) = deliver_inner(ctx, format) {
        log_hook_error(&ctx.store, "deliver", &e);
    }
    Ok(0)
}

fn deliver_inner(ctx: &Ctx, format: Option<&str>) -> Result<()> {
    if ctx.store.legacy_present() || !stdout_reaches_anyone() {
        return Ok(());
    }
    let Some(seat) = ctx.seat_here()? else {
        return Ok(());
    };
    ctx.bind(&seat)?;
    let mut claimed: Vec<(String, String)> = Vec::new();
    let give_back = |claimed: &[(String, String)]| {
        for (b, id) in claimed {
            ctx.store.mailbox(b).unclaim(id, How::Injected);
        }
    };
    let mut parts = Vec::new();
    let (mut bytes, mut more) = (0, 0);
    for b in ctx.my_mailboxes(&seat) {
        let mb = ctx.store.mailbox(&b);
        for id in mb.unread() {
            if parts.len() >= DELIVER_BODIES || bytes >= DELIVER_BYTES {
                more += 1;
                continue;
            }
            let path = match mb.claim(&id, How::Injected) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(e) => {
                    give_back(&claimed);
                    return Err(e);
                }
            };
            claimed.push((b.clone(), id));
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    give_back(&claimed);
                    return Err(Error::io(&path, e));
                }
            };
            // A headline-only message is already in the prompt: receipt, no text.
            if !Message::parse(&text).body_is_headline() {
                bytes += text.len();
                parts.push(text.trim_end_matches('\n').to_string());
            }
        }
    }
    if parts.is_empty() {
        return Ok(());
    }
    if more > 0 {
        parts.push(format!("({more} more unread: hail inbox prints them)"));
    }
    let text = parts.join("\n---\n");
    let out = match format {
        Some(_) => hooks::hook_json("UserPromptSubmit", &text),
        None => text,
    };
    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{out}")
        .and_then(|_| stdout.flush())
        .is_err()
    {
        // The output never reached the agent: give the messages back.
        give_back(&claimed);
    }
    Ok(())
}

/// Append one line to `hook-errors.log`, rotating it at 1 MB.
pub fn log_hook_error(store: &Store, verb: &str, e: &Error) {
    let path = store.root().join("hook-errors.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = fs::rename(&path, path.with_extension("log.1"));
    }
    let _ = fs::create_dir_all(store.root());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {verb}: {e}", crate::time::iso(crate::time::now()));
    }
}

/// A claim is only worth making when the output can reach the agent. Rust
/// reopens a closed fd 1 as /dev/null at startup, and writes there always
/// succeed, so a hook killed or redirected to /dev/null would lose its mail.
fn stdout_reaches_anyone() -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata("/dev/fd/1"), fs::metadata("/dev/null")) {
        (Err(_), _) => false,
        (Ok(out), Ok(null)) => out.rdev() != null.rdev() || out.ino() != null.ino(),
        (Ok(_), Err(_)) => true,
    }
}

pub fn inbox(ctx: &Ctx, peek: bool, all: bool) -> Result<u8> {
    let seat = ctx.require_seat()?;
    let mut shown = 0;
    for b in ctx.my_mailboxes(&seat) {
        let mb = ctx.store.mailbox(&b);
        let mut ids: Vec<String> = mb.unread();
        if all {
            ids.extend(mb.claimed().into_iter().map(|(id, _)| id));
            ids.sort();
        }
        for id in ids {
            let Some(found) = mb.find(&id) else { continue };
            if shown > 0 {
                outln!("---");
            }
            print_found(&found)?;
            shown += 1;
            if !peek && found.receipt.is_none() {
                mb.claim(&id, How::Read)?;
            }
        }
    }
    if shown == 0 {
        outln!("(inbox empty)");
    }
    Ok(0)
}

fn print_found(found: &Found) -> Result<()> {
    if let Some(r) = found.receipt {
        outln!("receipt: {r}");
    }
    let text = fs::read_to_string(&found.path).map_err(|e| Error::io(&found.path, e))?;
    out!("{text}");
    Ok(())
}

/// A message by id from any seat, without claiming it.
pub fn show(ctx: &Ctx, id: &str) -> Result<u8> {
    match locate(&ctx.store, id) {
        Some(found) => {
            print_found(&found)?;
            Ok(0)
        }
        None => Err(Error::State(format!(
            "no message with id {id}; check the id (hail brief lists recent ones)"
        ))),
    }
}

fn locate(store: &Store, id: &str) -> Option<Found> {
    if let Some(seat) = ids::lookup(store, id) {
        if let Some(f) = store.mailbox(&seat).find(id) {
            return Some(f);
        }
    }
    archived(store, id)
}

/// gc moves old mail to `archive/<yyyy-mm>/<seat>/` and notes it in `archive/index`.
fn archived(store: &Store, id: &str) -> Option<Found> {
    let index = fs::read_to_string(store.archive_dir().join("index")).ok()?;
    let line = index.lines().find(|l| l.split(' ').next() == Some(id))?;
    let mut f = line.split(' ');
    let (_, month, seat) = (f.next()?, f.next()?, f.next()?);
    crate::store::mailbox::Mailbox::new(store.archive_dir().join(month).join(seat)).find(id)
}

/// `delivered` | `injected <t>` | `read <t>` | `inline <t>` | `unknown`.
pub fn receipt_line(store: &Store, id: &str) -> String {
    match locate(store, id) {
        Some(Found {
            receipt: Some(r), ..
        }) => r.to_string(),
        Some(Found { receipt: None, .. }) => "delivered".into(),
        None => "unknown".into(),
    }
}

pub fn sent(ctx: &Ctx, id: &str) -> Result<u8> {
    outln!("{}", receipt_line(&ctx.store, id));
    Ok(0)
}

/// Block until every id (or any, with `--any`) has a receipt. The only verb
/// that waits. Polls the index every 100 ms: a few stats per id.
pub fn await_ids(ctx: &Ctx, ids: &[String], timeout: u64, any: bool) -> Result<u8> {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let has = |id: &String| {
        matches!(
            locate(&ctx.store, id),
            Some(Found {
                receipt: Some(_),
                ..
            })
        )
    };
    loop {
        let n = ids.iter().filter(|id| has(id)).count();
        let done = if any { n > 0 } else { n == ids.len() };
        let timed_out = Instant::now() >= deadline;
        if done || timed_out {
            let fallback = if done { "pending" } else { "timeout" };
            for id in ids {
                let line = receipt_line(&ctx.store, id);
                let shown = if line.starts_with("delivered") || line == "unknown" {
                    fallback.to_string()
                } else {
                    line
                };
                outln!("{id} {shown}");
            }
            return Ok(if done { 0 } else { 1 });
        }
        sleep(Duration::from_millis(100));
    }
}
