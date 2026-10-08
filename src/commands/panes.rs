//! Panes and seats: `list`, `seats`, `whoami`; driving a non-agent pane with
//! `read`, `type`, `keys`; and the old identity verbs, kept as
//! shims that never fail so older scripts and hooks keep working.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::out::Table;
use crate::route;
use crate::seat::Addr;
use crate::time;
use crate::transport::agent::Agent;
use crate::transport::pane_map::PaneMap;
use crate::transport::tmux::{Pane, Tmux};
use crate::transport::{Woken, type_verified};

fn tmux_and_panes(ctx: &Ctx) -> Result<(Tmux, PaneMap)> {
    let t = Tmux::detect()?;
    let pm = PaneMap::load(&t, ctx.home.as_deref())
        .map_err(|e| Error::State(format!("{e}; hail doctor says why tmux is unreachable")))?;
    Ok((t, pm))
}

pub fn whoami(ctx: &Ctx) -> Result<u8> {
    let seat = ctx.require_seat()?;
    outln!("seat: {}", seat.name);
    let boxes = ctx.mailboxes(&seat);
    if boxes.seat.is_some() {
        outln!(
            "mailbox: {} (shared directory; this pane's sub-seat)",
            boxes.primary
        );
    }
    outln!("from: {} ({})", seat.root.display(), seat.source.describe());
    outln!("state: {}", ctx.store.root().display());
    Ok(0)
}

pub fn list(ctx: &Ctx) -> Result<u8> {
    let (_, pm) = tmux_and_panes(ctx)?;
    let mut t = Table::new(&["TARGET", "SESSION:WIN", "SIZE", "PROCESS", "SEAT", "CWD"]);
    for p in &pm.panes {
        t.row(vec![
            p.id.clone(),
            format!("{}:{}", p.session, p.window),
            p.size.clone(),
            p.agent
                .as_ref()
                .map_or(p.command.as_str(), Agent::name)
                .to_string(),
            pm.seat_of(p).unwrap_or("-").to_string(),
            tilde(ctx, &p.path),
        ]);
    }
    t.print();
    Ok(0)
}

fn tilde(ctx: &Ctx, p: &Path) -> String {
    match ctx.home.as_deref().and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// Every seat (or one, with its sub-seats): agent panes, unread mail, open
/// obligations, root. Writes nothing.
pub fn seats(ctx: &Ctx, only: Option<&str>) -> Result<u8> {
    let pm = Tmux::detect()
        .ok()
        .and_then(|t| PaneMap::load(&t, ctx.home.as_deref()).ok());
    let mut addrs = ctx.store.mailboxes();
    addrs.extend(
        pm.as_ref()
            .map(PaneMap::seat_names)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| Addr::parse(s)),
    );
    addrs.sort();
    addrs.dedup();
    if let Some(o) = only {
        addrs.retain(|a| a.seat() == o);
        if addrs.is_empty() {
            return Err(Error::Usage(format!("no seat {o}; hail seats lists them")));
        }
    }
    let label = |p: &Pane| format!("{}:{}", p.id, p.agent.as_ref().map_or("-", Agent::name));
    let now = time::now();
    let mut t = Table::new(&["SEAT", "AGENTS", "UNREAD", "OWED", "HOOK", "ROOT"]);
    for addr in addrs {
        let agents = match (&pm, &addr) {
            (Some(pm), Addr::Seat(seat)) => pm
                .agents_in(seat)
                .iter()
                .map(|p| label(p))
                .collect::<Vec<_>>()
                .join(" "),
            (Some(pm), Addr::Sub { pane, .. }) => {
                pm.find(pane).map_or_else(|| "(pane gone)".into(), label)
            }
            (None, _) => "?".into(),
        };
        t.row(vec![
            addr.to_string(),
            if agents.is_empty() {
                "-".into()
            } else {
                agents
            },
            ctx.store.mailbox(&addr).unread().len().to_string(),
            ctx.store.owed(&addr).len().to_string(),
            // How long ago a prompt hook last read this mailbox: "-" means
            // nothing delivers its mail until someone runs hail inbox there.
            ctx.store
                .mailbox(&addr)
                .hooked_at()
                .map_or_else(|| "-".into(), |t| time::ago(now, t)),
            ctx.store
                .seat_root(addr.seat())
                .map(|r| tilde(ctx, &r))
                .unwrap_or_default(),
        ]);
    }
    t.print();
    Ok(0)
}

/// The read guard for driving a pane: `read` sets the mark, `type` and
/// `keys` require it and consume it, so nobody types into a pane unseen.
struct ReadMark(PathBuf);

impl ReadMark {
    fn of(ctx: &Ctx, pane: &str) -> Self {
        Self(ctx.store.root().join("read").join(pane.replace('%', "_")))
    }

    fn set(&self) {
        if let Some(d) = self.0.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(&self.0, "");
    }

    fn require(&self, pane: &str) -> Result<()> {
        if self.0.is_file() {
            Ok(())
        } else {
            Err(Error::State(format!(
                "read the pane before typing into it: hail read {pane}"
            )))
        }
    }

    fn consume(&self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn read(ctx: &Ctx, arg: &str, lines: usize) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = route::drive(ctx, arg, &pm, &t)?;
    // -S moves the start into scrollback but the capture still ends at the
    // bottom of the screen: drop the blank rows under the cursor, keep N.
    let text = t.capture(&p.id, Some(-i64::try_from(lines).unwrap_or(i64::MAX)))?;
    let all: Vec<&str> = text.lines().collect();
    let last = all
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);
    let kept = &all[..last];
    for l in &kept[kept.len().saturating_sub(lines)..] {
        outln!("{l}");
    }
    ReadMark::of(ctx, &p.id).set();
    Ok(0)
}

pub fn type_text(ctx: &Ctx, arg: &str, text: &str) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = route::drive(ctx, arg, &pm, &t)?;
    let mark = ReadMark::of(ctx, &p.id);
    mark.require(&p.id)?;
    if type_verified(&t, &p, text)? == Woken::NotConfirmed {
        return Err(Error::State(format!(
            "could not see the text in {0} after 10 s; it was typed once. hail read {0} to check",
            p.id
        )));
    }
    mark.consume();
    Ok(0)
}

pub fn keys(ctx: &Ctx, arg: &str, keys: &[String]) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    let p = route::drive(ctx, arg, &pm, &t)?;
    let mark = ReadMark::of(ctx, &p.id);
    mark.require(&p.id)?;
    for (i, k) in keys.iter().enumerate() {
        t.send_key(&p.id, k, i == 0 && p.in_mode)?;
    }
    mark.consume();
    Ok(0)
}

// --- old identity verbs: shims that never fail ------------------------------

/// `name` must not fail: seat.sh calls it from a `SessionStart` hook.
pub fn name_shim(ctx: &Ctx, target: Option<&str>) -> u8 {
    let here = ctx
        .seat_here()
        .ok()
        .flatten()
        .map_or_else(|| "(none here)".into(), |s| s.name);
    eprintln!(
        "hail: labels are gone in 0.4; {} is seat {here}, named by its directory (hail whoami)",
        target.unwrap_or("this pane")
    );
    0
}

pub fn hello_shim(ctx: &Ctx) -> Result<u8> {
    outln!("{}", ctx.require_seat()?.name);
    Ok(0)
}

pub fn resolve_shim(ctx: &Ctx, seat: &str) -> Result<u8> {
    let (t, pm) = tmux_and_panes(ctx)?;
    outln!("{}", route::drive(ctx, seat, &pm, &t)?.id);
    Ok(0)
}

pub fn id_shim(ctx: &Ctx) -> Result<u8> {
    let pane = ctx.tmux_pane.as_ref().ok_or_else(|| {
        Error::State("not running inside a tmux pane ($TMUX_PANE is unset)".into())
    })?;
    outln!("{pane}");
    Ok(0)
}
