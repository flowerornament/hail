//! `hail brief`: the standing state, a few lines. Bounded so it stays useful
//! a year from now: at most five entries per section unless `--all`.
//!
//! One write, by design: pending sends that have a receipt, or that lapsed,
//! leave the pending set here (spec §6.5).

use std::fmt::Write as _;
use std::fs;

use jiff::Timestamp;

use crate::ctx::{Boxes, Ctx};
use crate::envelope::{Kind, Tag, clip};
use crate::error::Result;
use crate::hooks;
use crate::policy::{BRIEF_SHOWN, PENDING_LAPSE, PENDING_LATE, secs};
use crate::seat::Addr;
use crate::store::Status;
use crate::store::message::Message;
use crate::store::records::Entry;
use crate::time;

pub fn run(ctx: &Ctx, all: bool, hook: bool) -> Result<u8> {
    if !hook {
        return brief(ctx, all, false).map(|()| 0);
    }
    // A session-start hook never fails a session: errors go to the log.
    if let Err(e) = brief(ctx, all, true) {
        hooks::log_error(&ctx.store, "brief", &e);
    }
    Ok(0)
}

/// One brief section: a title with a count, then the lines, bounded.
struct Section {
    title: &'static str,
    /// Word after the count: `inbox (3 unread)`.
    unit: &'static str,
    lines: Vec<String>,
    /// Age of the oldest entry, shown when some are cut.
    oldest_days: Option<i64>,
}

fn brief(ctx: &Ctx, all: bool, hook: bool) -> Result<()> {
    if ctx.store.legacy_present() {
        outln!("hail 0.4 is installed but the 0.3 state is not migrated yet: run hail migrate");
        return Ok(());
    }
    let seat = match ctx.seat_here()? {
        Some(s) => s,
        None if hook => return Ok(()),
        None => return ctx.require_seat().map(|_| ()),
    };
    ctx.bind(&seat)?;
    let boxes = ctx.mailboxes(&seat);
    let now = time::now();
    let cap = if all { usize::MAX } else { BRIEF_SHOWN };
    let width = ctx.max + 80;

    let mut out = String::new();
    write_section(&mut out, &unread(ctx, &boxes), cap, width);
    let (late, lapsed) = late_sends(ctx, &boxes, now);
    write_section(&mut out, &late, cap, width);
    if lapsed > 0 {
        let _ = writeln!(
            out,
            "{lapsed} send(s) lapsed after 7 days without a receipt"
        );
    }
    let (holds, others) = holds(ctx, &boxes.primary, all, now);
    write_section(&mut out, &holds, cap, width);
    if others > 0 {
        let _ = writeln!(
            out,
            "{others} other holds in effect between other seats (hail brief --all)"
        );
    }
    write_section(&mut out, &owed(ctx, &boxes, now), cap, width);
    out!("{out}");
    Ok(())
}

/// Unread mail in this pane's sub-seat and the seat, as envelope lines.
fn unread(ctx: &Ctx, boxes: &Boxes) -> Section {
    let lines = boxes
        .iter()
        .flat_map(|addr| {
            let mb = ctx.store.mailbox(addr);
            mb.unread()
                .into_iter()
                .filter_map(move |id| fs::read_to_string(mb.find(&id)?.path).ok())
        })
        .map(|text| {
            let m = Message::parse(&text);
            let from = m
                .get("from")
                .unwrap_or("?")
                .split('/')
                .next()
                .unwrap_or("?");
            Tag::brief(m.get("kind").unwrap_or("?"))
                .field("from", from)
                .field("id", m.get("id").unwrap_or("?"))
                .opt("bead", m.get("bead").and_then(|b| b.split(' ').next()))
                .opt("re", m.get("re"))
                .opt("scope", m.get("scope"))
                .text(m.get("ask").unwrap_or(""))
        })
        .collect();
    Section {
        title: "inbox",
        unit: " unread",
        lines,
        oldest_days: None,
    }
}

/// My sends with no receipt after a while. A send with a receipt, or one
/// that lapsed, leaves the pending set; the count is those that lapsed.
fn late_sends(ctx: &Ctx, boxes: &Boxes, now: Timestamp) -> (Section, usize) {
    let mut lines = Vec::new();
    let mut lapsed = 0;
    for owner in boxes.iter() {
        for p in ctx.store.pending(owner) {
            let delivered = ctx.store.status(&p.id) == Status::Delivered;
            let age = p.sent.map_or(i64::MAX, |t| now.as_second() - t.as_second());
            if !delivered || age > secs(PENDING_LAPSE) {
                lapsed += usize::from(delivered);
                let _ = ctx.store.remove_pending(owner, &p.id);
            } else if age >= secs(PENDING_LATE) {
                let line = Tag::brief(p.kind.map_or("?", Kind::as_str))
                    .field("to", &p.to)
                    .field("id", &p.id)
                    .text(&p.headline);
                lines.push(format!("{line} ({}m, no receipt)", age / 60));
            }
        }
    }
    (
        Section {
            title: "my sends without receipt",
            unit: "",
            lines,
            oldest_days: None,
        },
        lapsed,
    )
}

/// Holds sent to or by this seat in full (all with `--all`), and how many
/// others are in effect: 0.3-era holds were sent to each seat one by one.
fn holds(ctx: &Ctx, me: &Addr, all: bool, now: Timestamp) -> (Section, usize) {
    let mine = |e: &Entry| {
        [&e.to, &e.issuer]
            .iter()
            .any(|a| *a == me || a.seat() == me.seat())
    };
    let (shown, others): (Vec<_>, Vec<_>) =
        ctx.store.holds().into_iter().partition(|e| all || mine(e));
    let oldest_days = shown.last().map(|e| age_days(e, now));
    let lines = shown
        .iter()
        .map(|e| {
            let line = Tag::brief(e.kind.map_or("hold", Kind::as_str))
                .field("from", &e.issuer)
                .field("to", &e.to)
                .field("id", &e.id)
                .opt("scope", e.scope.as_deref())
                .text(&e.headline);
            format!(
                "{line} (since {})",
                e.time.map_or_else(|| "?".into(), time::iso)
            )
        })
        .collect();
    (
        Section {
            title: "holds / blocks on me",
            unit: "",
            lines,
            oldest_days,
        },
        others.len(),
    )
}

/// Obligations on this pane's sub-seat and the seat, newest first, each with
/// the command that closes it.
fn owed(ctx: &Ctx, boxes: &Boxes, now: Timestamp) -> Section {
    let mut owed: Vec<Entry> = boxes.iter().flat_map(|b| ctx.store.owed(b)).collect();
    owed.sort_by(|a, b| b.id.cmp(&a.id));
    let oldest_days = owed.last().map(|e| age_days(e, now));
    let lines = owed
        .iter()
        .map(|e| {
            let line = Tag::brief(e.kind.map_or("?", Kind::as_str))
                .field("from", &e.issuer)
                .field("id", &e.id)
                .opt("scope", e.scope.as_deref())
                .text(&e.headline);
            format!(
                "{line} — hail {} done --re {} '<what was done>'",
                e.issuer, e.id
            )
        })
        .collect();
    Section {
        title: "open obligations on me",
        unit: "",
        lines,
        oldest_days,
    }
}

fn write_section(out: &mut String, s: &Section, cap: usize, width: usize) {
    let n = s.lines.len();
    if n == 0 {
        return;
    }
    let _ = writeln!(out, "{} ({n}{})", s.title, s.unit);
    for l in s.lines.iter().take(cap) {
        let _ = writeln!(out, "  {}", clip(l, width));
    }
    if n > cap {
        let oldest = s
            .oldest_days
            .map(|d| format!(", oldest {d}d"))
            .unwrap_or_default();
        let _ = writeln!(out, "  … {} more{oldest} (hail brief --all)", n - cap);
    }
}

fn age_days(e: &Entry, now: Timestamp) -> i64 {
    e.time
        .map_or(0, |t| (now.as_second() - t.as_second()) / 86400)
}
