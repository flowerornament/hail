//! Panes and seats: `list`, `seats`, `whoami`; driving a non-agent pane with
//! `read`, `type`, `keys`; and the 0.3 identity verbs kept as shims for 0.4.

use std::fs;
use std::path::PathBuf;

use crate::ctx::{Ctx, PaneMap, looks_like_pane};
use crate::error::{Error, Result};
use crate::seat;
use crate::transport::Woken;
use crate::transport::tmux::{Pane, Tmux};
use crate::transport::type_verified;

fn tmux_and_panes(ctx: &Ctx) -> Result<(Tmux, PaneMap)> {
    let t = Tmux::detect()?;
    let pm = PaneMap::load(&t, ctx.home.as_deref())
        .map_err(|e| Error::State(format!("{e}; hail doctor says why tmux is unreachable")))?;
    Ok((t, pm))
}

pub fn whoami(ctx: &Ctx) -> Result<u8> {
    let seat = ctx.require_seat()?;
    outln!("seat: {}", seat.name);
    let mailbox = ctx.my_mailbox(&seat);
    if mailbox != seat.name {
        outln!("mailbox: {mailbox} (shared directory; this pane's sub-seat)");
    }
    outln!("from: {} ({})", seat.root.display(), seat.source.describe());
    outln!("state: {}", ctx.store.root().display());
    Ok(0)
}

pub fn list(ctx: &Ctx) -> Result<u8> {
    let (_, pm) = tmux_and_panes(ctx)?;
    outln!(
        "{:<8} {:<16} {:<10} {:<10} {:<20} CWD",
        "TARGET",
        "SESSION:WIN",
        "SIZE",
        "PROCESS",
        "SEAT"
    );
    for p in &pm.panes {
        let process = p.agent.clone().unwrap_or_else(|| p.command.clone());
        outln!(
            "{:<8} {:<16} {:<10} {:<10} {:<20} {}",
            p.id,
            format!("{}:{}", p.session, p.window),
            p.size,
            process,
            pm.seat_of(p).unwrap_or("-"),
            tilde(ctx, &p.path)
        );
    }
    Ok(0)
}

fn tilde(ctx: &Ctx, p: &std::path::Path) -> String {
    match ctx.home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// Every seat: agent panes, unread mail, open obligations.
pub fn seats(ctx: &Ctx, only: Option<&str>) -> Result<u8> {
    let pm = Tmux::detect()
        .ok()
        .and_then(|t| PaneMap::load(&t, ctx.home.as_deref()).ok());
    let mut names = ctx.store.seat_names();
    if let Some(pm) = &pm {
        names.extend(pm.seat_names());
    }
    names.sort();
    names.dedup();
    if let Some(o) = only {
        names.retain(|n| n == o || n.starts_with(&format!("{o}@")));
        if names.is_empty() {
            return Err(Error::Usage(format!("no seat {o}; hail seats lists them")));
        }
    }
    outln!(
        "{:<24} {:<22} {:>6} {:>5}  ROOT",
        "SEAT",
        "AGENTS",
        "UNREAD",
        "OWED"
    );
    for n in names {
        let (base, sub) = seat::split_sub_seat(&n);
        let agents = match (&pm, sub) {
            (Some(pm), None) => {
                let a = pm.agents_in(base);
                crate::ctx::note_sharing(&ctx.store, base, &a);
                a.iter()
                    .map(|p| format!("{}:{}", p.id, p.agent.as_deref().unwrap_or("?")))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            (Some(pm), Some(id)) => match pm.find(id) {
                Some(p) => format!("{}:{}", p.id, p.agent.as_deref().unwrap_or("-")),
                None => "(pane gone)".into(),
            },
            (None, _) => "?".into(),
        };
        let unread = ctx.store.mailbox(&n).unread().len();
        let owed = ctx
            .store
            .records(crate::store::records::Kind::Owed, &n)
            .len();
        let root = ctx
            .store
            .seat_root(base)
            .map(|r| tilde(ctx, &r))
            .unwrap_or_default();
        outln!(
            "{:<24} {:<22} {:>6} {:>5}  {}",
            n,
            if agents.is_empty() {
                "-".into()
            } else {
                agents
            },
            unread,
            owed,
            root
        );
    }
    Ok(0)
}

/// A pane for `read`/`type`/`keys`: a tmux target, a sub-seat's pane, or a
/// seat's single agent pane (else its single pane).
fn target_pane(ctx: &Ctx, t: &Tmux, pm: &PaneMap, arg: &str) -> Result<Pane> {
    if let (base, Some(id)) = seat::split_sub_seat(arg) {
        return pm
            .find(id)
            .filter(|p| pm.seat_of(p) == Some(base))
            .cloned()
            .ok_or_else(|| Error::Usage(format!("no pane {id} in seat {base}")));
    }
    let in_seat = pm.in_seat(arg);
    if !in_seat.is_empty() {
        let agents: Vec<&&Pane> = in_seat.iter().filter(|p| p.agent.is_some()).collect();
        return match (agents.len(), in_seat.len()) {
            (1, _) => Ok((**agents[0]).clone()),
            (0, 1) => Ok(in_seat[0].clone()),
            _ => Err(Error::Seat(format!(
                "seat {arg} has several panes ({}); name one",
                in_seat
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ))),
        };
    }
    if looks_like_pane(arg) {
        let id = t.pane_id(arg)?;
        return pm
            .find(&id)
            .cloned()
            .ok_or_else(|| Error::Usage(format!("no pane {arg}")));
    }
    let _ = ctx;
    Err(Error::Usage(format!(
        "no pane or seat '{arg}'; hail list shows panes"
    )))
}

fn read_mark(ctx: &Ctx, pane: &str) -> PathBuf {
    ctx.store.root().join("read").join(pane.replace('%', "_"))
}

pub fn read(ctx: &Ctx, arg: &str, lines: usize) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = target_pane(ctx, &t, &pm, arg)?;
    // -S moves the start into scrollback but the capture still ends at the
    // bottom of the screen: drop the blank rows under the cursor, keep N.
    let text = t.capture(&p.id, Some(-(lines as i64)))?;
    let all: Vec<&str> = text.lines().collect();
    let last = all
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    let kept = &all[..last];
    for l in &kept[kept.len().saturating_sub(lines)..] {
        outln!("{l}");
    }
    let mark = read_mark(ctx, &p.id);
    if let Some(d) = mark.parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(mark, "");
    Ok(0)
}

fn require_read(ctx: &Ctx, pane: &str) -> Result<()> {
    if read_mark(ctx, pane).is_file() {
        Ok(())
    } else {
        Err(Error::State(format!(
            "read the pane before typing into it: hail read {pane}"
        )))
    }
}

pub fn type_text(ctx: &Ctx, arg: &str, text: &str) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = target_pane(ctx, &t, &pm, arg)?;
    require_read(ctx, &p.id)?;
    if type_verified(&t, &p, text)? == Woken::NotConfirmed {
        return Err(Error::State(format!(
            "could not see the text in {} after 2 s; it was typed once. hail read {} to check",
            p.id, p.id
        )));
    }
    let _ = fs::remove_file(read_mark(ctx, &p.id));
    Ok(0)
}

pub fn keys(ctx: &Ctx, arg: &str, keys: &[String]) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = target_pane(ctx, &t, &pm, arg)?;
    require_read(ctx, &p.id)?;
    for (i, k) in keys.iter().enumerate() {
        t.send_key(&p.id, k, i == 0 && p.in_mode)?;
    }
    let _ = fs::remove_file(read_mark(ctx, &p.id));
    Ok(0)
}

// --- 0.3 identity verbs, kept as shims through 0.4 ---------------------------

/// `name` must not fail: seat.sh calls it from a SessionStart hook.
pub fn name_shim(ctx: &Ctx, target: Option<&str>) -> Result<u8> {
    let here = ctx
        .seat_here()
        .ok()
        .flatten()
        .map(|s| s.name)
        .unwrap_or_else(|| "(none here)".into());
    eprintln!(
        "hail: labels are gone in 0.4; {} is seat {here}, named by its directory (hail whoami)",
        target.unwrap_or("this pane")
    );
    Ok(0)
}

pub fn hello_shim(ctx: &Ctx) -> Result<u8> {
    outln!("{}", ctx.require_seat()?.name);
    Ok(0)
}

pub fn resolve_shim(ctx: &Ctx, seat_name: &str) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = target_pane(ctx, &t, &pm, seat_name)?;
    outln!("{}", p.id);
    Ok(0)
}

pub fn id_shim(ctx: &Ctx) -> Result<u8> {
    match &ctx.tmux_pane {
        Some(p) => {
            outln!("{p}");
            Ok(0)
        }
        None => Err(Error::State(
            "not running inside a tmux pane ($TMUX_PANE is unset)".into(),
        )),
    }
}
