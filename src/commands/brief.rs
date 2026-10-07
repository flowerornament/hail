//! `hail brief`: the standing state, a few lines. Bounded so it stays useful
//! a year from now: at most five entries per section unless `--all`.

use std::fmt::Write as _;
use std::fs;

use crate::ctx::Ctx;
use crate::envelope::clip;
use crate::error::Result;
use crate::store::message::Message;
use crate::store::records::Kind;
use crate::time;

const SHOWN: usize = 5;
const PENDING_AFTER_SECS: i64 = 120;
const PENDING_EXPIRE_SECS: i64 = 7 * 24 * 3600;

pub fn run(ctx: &Ctx, all: bool, hook: bool) -> Result<u8> {
    if !hook {
        return brief(ctx, all, false);
    }
    // A session-start hook never fails a session: errors go to the log.
    if let Err(e) = brief(ctx, all, true) {
        super::receive::log_hook_error(&ctx.store, "brief", &e);
    }
    Ok(0)
}

fn brief(ctx: &Ctx, all: bool, hook: bool) -> Result<u8> {
    if ctx.store.legacy_present() {
        outln!("hail 0.4 is installed but the 0.3 state is not migrated yet: run hail migrate");
        return Ok(0);
    }
    let seat = match ctx.seat_here()? {
        Some(s) => s,
        None if hook => return Ok(0),
        None => return ctx.require_seat().map(|_| 0),
    };
    ctx.bind(&seat)?;
    let boxes = ctx.my_mailboxes(&seat);
    let me = boxes[0].clone();
    let width = ctx.max + 80;
    let cap = if all { usize::MAX } else { SHOWN };
    let now = time::now();
    let mut out = String::new();

    // Unread mail, in this pane's sub-seat and the seat.
    let mut unread: Vec<Message> = Vec::new();
    for b in &boxes {
        let mb = ctx.store.mailbox(b);
        unread.extend(
            mb.unread()
                .iter()
                .filter_map(|id| mb.find(id))
                .filter_map(|f| fs::read_to_string(&f.path).ok())
                .map(|t| Message::parse(&t)),
        );
    }
    section(
        &mut out,
        "inbox",
        "unread",
        unread.iter().map(envelope_line).collect(),
        cap,
        width,
        None,
    );

    // My sends without a receipt after two minutes; expire after a week.
    let mut late = Vec::new();
    let mut expired = 0;
    let pending = boxes.iter().flat_map(|b| {
        ctx.store
            .records(Kind::Pending, b)
            .into_iter()
            .map(move |(id, r)| (b.clone(), id, r))
    });
    for (owner, id, rec) in pending.collect::<Vec<_>>() {
        let receipt = super::receive::receipt_line(&ctx.store, &id);
        let age = now.as_second() - rec.get("epoch").and_then(|e| e.parse().ok()).unwrap_or(0);
        if receipt != "delivered" || age > PENDING_EXPIRE_SECS {
            if receipt == "delivered" {
                expired += 1;
            }
            let _ = ctx.store.remove_record(Kind::Pending, &owner, &id);
            continue;
        }
        if age >= PENDING_AFTER_SECS {
            late.push(format!(
                "[hail {} to:{} id:{id}] {} ({}m, no receipt)",
                rec.get("kind").unwrap_or("?"),
                rec.get("to").unwrap_or("?"),
                rec.get("headline").unwrap_or(""),
                age / 60
            ));
        }
    }
    section(
        &mut out,
        "my sends without receipt",
        "",
        late,
        cap,
        width,
        None,
    );
    if expired > 0 {
        let _ = writeln!(
            out,
            "{expired} send(s) expired after 7 days without a receipt"
        );
    }

    // Holds and blocks: mine (sent to me or by me) in full; others as a
    // count, since 0.3-era holds were sent to each seat one by one.
    let (base, _) = crate::seat::split_sub_seat(&me);
    let is_mine = |r: &Message| {
        [r.get("to"), r.get("issuer")]
            .into_iter()
            .flatten()
            .any(|s| s == me || crate::seat::split_sub_seat(s).0 == base)
    };
    let holds: Vec<(String, Message)> = ctx.store.records(Kind::Holds, "");
    let (mine, others): (Vec<_>, Vec<_>) = holds.iter().partition(|(_, r)| all || is_mine(r));
    let oldest = mine.last().map(|(_, r)| age_days(r, now));
    let lines = mine
        .iter()
        .map(|(id, r)| {
            format!(
                "[hail {} from:{} to:{} id:{id}{}] {} (since {})",
                r.get("kind").unwrap_or("hold"),
                r.get("issuer").unwrap_or("?"),
                r.get("to").unwrap_or("?"),
                scope(r),
                r.get("headline").unwrap_or(""),
                r.get("time").unwrap_or("?")
            )
        })
        .collect();
    section(
        &mut out,
        "holds / blocks on me",
        "",
        lines,
        cap,
        width,
        oldest,
    );
    if !others.is_empty() {
        let _ = writeln!(
            out,
            "{} other holds in effect between other seats (hail brief --all)",
            others.len()
        );
    }

    // Obligations on me.
    let mut owed: Vec<(String, Message)> = boxes
        .iter()
        .flat_map(|b| ctx.store.records(Kind::Owed, b))
        .collect();
    owed.sort_by(|a, b| b.0.cmp(&a.0));
    let oldest = owed.last().map(|(_, r)| age_days(r, now));
    let lines = owed
        .iter()
        .map(|(id, r)| {
            let issuer = r.get("issuer").unwrap_or("?");
            format!(
                "[hail {} from:{issuer} id:{id}{}] {} — hail {issuer} done --re {id} '<what was done>'",
                r.get("kind").unwrap_or("?"),
                scope(r),
                r.get("headline").unwrap_or("")
            )
        })
        .collect();
    section(
        &mut out,
        "open obligations on me",
        "",
        lines,
        cap,
        width,
        oldest,
    );

    out!("{out}");
    Ok(0)
}

fn section(
    out: &mut String,
    title: &str,
    word: &str,
    lines: Vec<String>,
    cap: usize,
    width: usize,
    oldest_days: Option<i64>,
) {
    if lines.is_empty() {
        return;
    }
    let n = lines.len();
    let label = if word.is_empty() {
        format!("{title} ({n})")
    } else {
        format!("{title} ({n} {word})")
    };
    let _ = writeln!(out, "{label}");
    for l in lines.iter().take(cap) {
        let _ = writeln!(out, "  {}", clip(l, width));
    }
    if n > cap {
        let oldest = oldest_days
            .map(|d| format!(", oldest {d}d"))
            .unwrap_or_default();
        let _ = writeln!(out, "  … {} more{oldest} (hail brief --all)", n - cap);
    }
}

fn envelope_line(m: &Message) -> String {
    let from = m
        .get("from")
        .unwrap_or("?")
        .split('/')
        .next()
        .unwrap_or("?");
    let bead = m
        .get("bead")
        .map(|b| format!(" bead:{}", b.split(' ').next().unwrap_or(b)))
        .unwrap_or_default();
    let re = m.get("re").map(|r| format!(" re:{r}")).unwrap_or_default();
    format!(
        "[hail {} from:{from} id:{}{bead}{re}{}] {}",
        m.get("kind").unwrap_or("?"),
        m.get("id").unwrap_or("?"),
        scope(m),
        m.get("ask").unwrap_or("")
    )
}

fn scope(m: &Message) -> String {
    m.get("scope")
        .filter(|s| !s.is_empty())
        .map(|s| format!(" scope:{s}"))
        .unwrap_or_default()
}

fn age_days(r: &Message, now: jiff::Timestamp) -> i64 {
    r.get("time")
        .and_then(time::parse_iso)
        .map(|t| (now.as_second() - t.as_second()) / 86400)
        .unwrap_or(0)
}
