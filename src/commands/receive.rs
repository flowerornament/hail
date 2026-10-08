//! Receiving: `deliver` (hooks), `inbox`, `show`; receipts: `sent`, `await`.
//! None of these runs a subprocess, because hooks run them on every prompt
//! and one fork costs more than the whole budget; keep bd and tmux out.

use std::fs;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::ctx::Ctx;
use crate::envelope::{self, Head, Kind};
use crate::error::{Error, Result};
use crate::hooks;
use crate::policy::{DELIVER_BODIES, DELIVER_BYTES};
use crate::seat::Addr;
use crate::store::ids::Id;
use crate::store::mailbox::{Found, How};
use crate::store::message::Message;
use crate::store::{Status, Store};

/// Print this seat's unread bodies and claim them (`injected`). A hook must
/// never fail a session: this always exits 0, and an error is logged to
/// `hook-errors.log` with every claim it made given back.
pub fn deliver(ctx: &Ctx, format: Option<&str>) -> u8 {
    if let Err(e) = deliver_inner(ctx, format) {
        hooks::log_error(&ctx.store, "deliver", &e);
    }
    0
}

fn deliver_inner(ctx: &Ctx, format: Option<&str>) -> Result<()> {
    if ctx.store.legacy_present() || !hooks::stdout_reaches_anyone() {
        return Ok(());
    }
    let Some(seat) = ctx.seat_here()? else {
        return Ok(());
    };
    ctx.bind(&seat)?;
    let boxes = ctx.mailboxes(&seat);
    // Only an installed hook passes --format: a `hail deliver` typed by hand
    // must not convince senders that this seat's hooks run.
    if format.is_some() {
        for addr in boxes.iter() {
            ctx.store.mailbox(addr).touch_hooked();
        }
    }
    let mut claims = Claims {
        store: &ctx.store,
        made: Vec::new(),
    };
    let mut parts = Vec::new();
    let (mut bytes, mut more) = (0, 0);
    for addr in boxes.iter() {
        for id in ctx.store.mailbox(addr).unread() {
            if parts.len() >= DELIVER_BODIES || bytes >= DELIVER_BYTES {
                more += 1;
                continue;
            }
            let Some(text) = claims.claim(addr, &id)? else {
                continue;
            };
            let msg = Message::parse(&text);
            // A headline-only message goes in as its envelope line, whether or
            // not the envelope was typed: the composer can lose typed text (a
            // dialog, a cleared prompt), and a repeated line costs less than
            // a lost message.
            let text = match envelope_line(&msg) {
                Some(line) if msg.body_is_headline() => line,
                _ => text.trim_end_matches('\n').to_string(),
            };
            bytes += text.len();
            parts.push(text);
        }
    }
    if parts.is_empty() {
        claims.keep();
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
        .and_then(|()| stdout.flush())
        .is_ok()
    {
        claims.keep();
    }
    Ok(())
}

/// The one-line envelope a message was (or would have been) typed as.
fn envelope_line(m: &Message) -> Option<String> {
    let head = Head {
        kind: Kind::parse(m.get("kind")?)?,
        from: m.get("from")?,
        reply: m.get("reply")?,
        id: m.get("id")?,
        for_: m.get("for"),
        bead: m.get("bead"),
        re: m.get("re"),
        scope: m.get("scope"),
    };
    Some(envelope::render(&head, m.get("ask")?, false))
}

/// The claims one delivery made. Dropped without [`Claims::keep`] (an error,
/// or output that never reached the agent), they are given back: delivery
/// is at least once, never lost.
struct Claims<'a> {
    store: &'a Store,
    made: Vec<(Addr, Id)>,
}

impl Claims<'_> {
    /// Claim one message and read it; `None` when another claimer won.
    fn claim(&mut self, addr: &Addr, id: &Id) -> Result<Option<String>> {
        let Some(path) = self.store.mailbox(addr).claim(id, How::Injected)? else {
            return Ok(None);
        };
        self.made.push((addr.clone(), id.clone()));
        fs::read_to_string(&path)
            .map(Some)
            .map_err(Error::at(&path))
    }

    fn keep(mut self) {
        self.made.clear();
    }
}

impl Drop for Claims<'_> {
    fn drop(&mut self) {
        for (addr, id) in &self.made {
            self.store.mailbox(addr).unclaim(id, How::Injected);
        }
    }
}

pub fn inbox(ctx: &Ctx, peek: bool, all: bool) -> Result<u8> {
    let seat = ctx.require_seat()?;
    let mut shown = 0;
    for addr in ctx.mailboxes(&seat).iter() {
        let mb = ctx.store.mailbox(addr);
        let mut ids = mb.unread();
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
    let text = fs::read_to_string(&found.path).map_err(Error::at(&found.path))?;
    out!("{text}");
    Ok(())
}

fn parse_id(id: &str) -> Result<Id> {
    Id::parse(id).ok_or_else(|| Error::Usage(format!("'{id}' is not a message id")))
}

/// A message by id from any seat, without claiming it.
pub fn show(ctx: &Ctx, id: &str) -> Result<u8> {
    let found = ctx.store.find(&parse_id(id)?).ok_or_else(|| {
        Error::State(format!(
            "no message with id {id}; check the id (hail brief lists recent ones)"
        ))
    })?;
    print_found(&found)?;
    Ok(0)
}

pub fn sent(ctx: &Ctx, id: &str) -> u8 {
    let status = Id::parse(id).map_or(Status::Unknown, |id| ctx.store.status(&id));
    outln!("{status}");
    0
}

/// Block until every id (or any, with `--any`) has a receipt. The only verb
/// that waits. Polls the index every 100 ms: a few stats per id.
pub fn await_ids(ctx: &Ctx, ids: &[String], timeout: u64, any: bool) -> u8 {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = |id: &String| Id::parse(id).map_or(Status::Unknown, |id| ctx.store.status(&id));
    loop {
        let statuses: Vec<Status> = ids.iter().map(status).collect();
        let received = statuses
            .iter()
            .filter(|s| matches!(s, Status::Received(_)))
            .count();
        let done = if any {
            received > 0
        } else {
            received == ids.len()
        };
        if done || Instant::now() >= deadline {
            // An id without a receipt is "pending" when another satisfied
            // --any, else "timeout".
            let unmet = if done { "pending" } else { "timeout" };
            for (id, status) in ids.iter().zip(statuses) {
                if let Status::Received(r) = status {
                    outln!("{id} {r}");
                } else {
                    outln!("{id} {unmet}");
                }
            }
            return u8::from(!done);
        }
        sleep(Duration::from_millis(100));
    }
}
