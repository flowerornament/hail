//! `hail doctor`: one line per check, and every problem names its fix.

use crate::ctx::Ctx;
use crate::policy::GC_DUE;
use crate::seat::{Addr, Seat};
use crate::store::list_names;
use crate::transport::pane_map::PaneMap;
use crate::transport::tmux::Tmux;

use super::setup::{harnesses, plan};

enum Check {
    Ok(String),
    /// A problem; the line says how to fix it.
    Fix(String),
    /// Worth knowing, not a problem.
    Note(String),
}

pub fn run(ctx: &Ctx) -> u8 {
    let seat = ctx.seat_here();
    let mut checks = vec![Check::Ok(format!(
        "hail {} — state {}",
        env!("CARGO_PKG_VERSION"),
        ctx.store.root().display()
    ))];
    if ctx.store.legacy_present() {
        checks.push(Check::Fix(
            "0.3 state is not migrated: run hail migrate".into(),
        ));
    }
    checks.push(match &seat {
        // Only compare: doctor is often run from the wrong directory, and
        // must not claim a seat name there.
        Ok(Some(s)) => match ctx.check_binding(s) {
            Ok(()) => Check::Ok(format!(
                "seat here: {} ({}, {})",
                s.name,
                s.source.describe(),
                s.root.display()
            )),
            Err(e) => Check::Fix(e.to_string()),
        },
        Ok(None) => Check::Ok(format!(
            "no seat here ({}); fine outside a workspace",
            ctx.cwd.display()
        )),
        Err(e) => Check::Fix(e.to_string()),
    });
    checks.extend(tmux_checks(ctx, seat.ok().flatten().as_ref()));
    checks.extend(hook_checks());
    checks.extend(size_checks(ctx));

    let mut problems = 0;
    for c in &checks {
        match c {
            Check::Ok(l) => outln!("ok   {l}"),
            Check::Fix(l) => {
                problems += 1;
                outln!("FIX  {l}");
            }
            Check::Note(l) => outln!("note {l}"),
        }
    }
    u8::from(problems > 0)
}

/// The server, this seat's wake pane, and unread mail nobody will pick up.
fn tmux_checks(ctx: &Ctx, seat: Option<&Seat>) -> Vec<Check> {
    let t = match Tmux::detect() {
        Ok(t) => t,
        Err(e) => return vec![Check::Fix(e.to_string())],
    };
    let pm = match PaneMap::load(&t, ctx.home.as_deref()) {
        Ok(pm) => pm,
        Err(e) => {
            return vec![Check::Fix(format!(
                "tmux ({}): {e}; start tmux or set HAIL_SOCKET",
                t.source
            ))];
        }
    };
    let agents = pm.panes.iter().filter(|p| p.agent.is_some()).count();
    let mut checks = vec![Check::Ok(format!(
        "tmux ({}): {} panes, {agents} agents",
        t.source,
        pm.panes.len()
    ))];
    if let Some(s) = seat {
        let ids: Vec<&str> = pm
            .agents_in(&s.name)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        checks.push(Check::Ok(match ids.as_slice() {
            [] => format!("no agent pane in seat {} (mail waits for the next session)", s.name),
            [one] => format!("wake pane for {}: {one}", s.name),
            many => format!(
                "seat {0} is shared by {1} agents ({2}): each Claude pane is addressed as {0}@<pane>",
                s.name,
                many.len(),
                many.join(" ")
            ),
        }));
    }
    // A sub-seat whose pane is gone is stranded; a seat no pane sits in is
    // normal for an agent that is off, and listed so nothing waits unseen.
    let mut waiting = Vec::new();
    for addr in ctx.store.mailboxes() {
        let unread = ctx.store.mailbox(&addr).unread().len();
        match &addr {
            _ if unread == 0 => {}
            Addr::Sub { pane, .. } if pm.find(pane).is_none() => checks.push(Check::Fix(format!(
                "orphaned sub-seat {addr} holds {unread} unread (its pane is gone): hail show <id> reads them"
            ))),
            Addr::Seat(seat) if pm.in_seat(seat).is_empty() => waiting.push(format!("{addr} ({unread})")),
            _ => {}
        }
    }
    if !waiting.is_empty() {
        checks.push(Check::Note(format!(
            "unread mail waits in seats with no pane now: {}",
            waiting.join(", ")
        )));
    }
    checks
}

fn hook_checks() -> Vec<Check> {
    harnesses()
        .into_iter()
        .filter(super::setup::Harness::installed)
        .map(|h| match plan(&h) {
            Ok(None) => Check::Ok(format!("{} hooks current ({})", h.name, h.file.display())),
            Ok(Some(_)) => Check::Fix(format!(
                "{} hooks missing or outdated in {}: run hail setup",
                h.name,
                h.file.display()
            )),
            Err(e) => Check::Fix(e.to_string()),
        })
        .collect()
}

fn size_checks(ctx: &Ctx) -> Vec<Check> {
    ctx.store
        .mailboxes()
        .into_iter()
        .filter_map(|addr| {
            let n = list_names(&ctx.store.seat_dir(&addr).join("cur")).len();
            (n > GC_DUE)
                .then(|| Check::Fix(format!("seat {addr} keeps {n} read messages: run hail gc")))
        })
        .collect()
}
