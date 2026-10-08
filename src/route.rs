//! Routing: what a target names, which mailbox mail for it goes to, and
//! which pane to type into. One parse ([`name`]) feeds two policies, because
//! the verbs want different things from the same name:
//!
//! - **Mail** ([`mail`], for sends): a message goes to a mailbox, and the
//!   pane woken is the seat's one agent. A pane id names its seat's mailbox,
//!   or its sub-seat when several agents share the directory. A shell is
//!   never woken: typing an envelope into a shell runs it as a command.
//! - **Drive** ([`drive`], for `read`, `type`, `keys`): the target is a pane
//!   to look at or type into, and driving a shell is the point. A pane id is
//!   that pane; a seat is its one agent pane, else its one pane.
//!
//! Both refuse a seat shared by several agents and list its sub-seats.
//!
//! The seat of any directory comes from `seat.rs` and a process's mailboxes
//! from `ctx.rs`; [`sender`] here checks, against the live panes, whether a
//! sending Claude pane signs as its sub-seat.

use std::path::{Path, PathBuf};

use crate::ctx::{Boxes, Ctx};
use crate::error::{Error, Result};
use crate::seat::{self, Addr, Address, Seat};
use crate::transport::pane_map::PaneMap;
use crate::transport::tmux::{Pane, Tmux};

/// What a target argument names.
enum Named<'a> {
    /// `hail@%28`: one Claude pane's sub-seat; the pane is checked.
    Sub(Addr, &'a Pane),
    /// A seat by name.
    Seat(&'a str),
    /// A tmux target (`%7`, `sess:1.2`), resolved to its pane.
    Pane(Pane),
}

/// Where mail goes, and the pane to wake if there is one.
pub struct Mail {
    pub to: Addr,
    /// The sub-agent this is for (`seat/name`): the parent relays it.
    pub for_: Option<String>,
    pub wake: Option<Pane>,
    /// Why nothing will be typed, when `wake` is `None`.
    pub no_wake: Option<String>,
}

/// Who is sending: the mailboxes it reads (the first is what it signs as)
/// and its pane when known.
pub struct Sender {
    pub boxes: Boxes,
    pub pane: Option<String>,
}

impl Sender {
    /// `from:` in the envelope: the mailbox, and the pane when it adds
    /// something (a sub-seat already names its pane).
    pub fn from(&self) -> String {
        match (&self.boxes.primary, &self.pane) {
            (Addr::Seat(s), Some(p)) => format!("{s}/{p}"),
            (addr, _) => addr.to_string(),
        }
    }
}

/// `%5`, `sess:1.2`, `3`: a tmux target rather than a seat name.
pub fn looks_like_pane(arg: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    arg.strip_prefix('%').is_some_and(digits) || arg.contains(':') || digits(arg)
}

/// Whether this pane may hold a sub-seat (see [`crate::transport::agent::Agent::can_hold_sub_seat`]).
pub fn can_hold_sub_seat(p: &Pane) -> bool {
    p.agent
        .as_ref()
        .is_some_and(crate::transport::agent::Agent::can_hold_sub_seat)
}

fn name<'a>(
    ctx: &Ctx,
    arg: &'a str,
    pm: Option<&'a PaneMap>,
    tmux: Option<&Tmux>,
) -> Result<Named<'a>> {
    let no_tmux = || {
        Error::State(format!(
            "{arg}: no tmux server answers; hail doctor says why"
        ))
    };
    let Some(addr) = Addr::parse(arg) else {
        return Err(not_an_address(arg));
    };
    if let Addr::Sub { seat, pane } = &addr {
        let pm = pm.ok_or_else(no_tmux)?;
        let p = pm.find(pane).ok_or_else(|| {
            Error::Seat(format!(
                "{arg}: pane {pane} is gone; hail seats lists live seats"
            ))
        })?;
        if pm.seat_of(p) != Some(seat.as_str()) || !can_hold_sub_seat(p) {
            return Err(Error::Seat(format!(
                "{arg}: {pane} is not a Claude pane in seat {seat}; hail seats lists seats"
            )));
        }
        return Ok(Named::Sub(addr.clone(), p));
    }
    // A seat is known once it has a mailbox (an agent ran hail there, or mail
    // was migrated to it), or while a pane sits in it.
    let known =
        ctx.store.seat_dir(&addr).is_dir() || pm.is_some_and(|pm| !pm.in_seat(arg).is_empty());
    if known {
        return Ok(Named::Seat(arg));
    }
    if looks_like_pane(arg) {
        let (pm, t) = (pm.ok_or_else(no_tmux)?, tmux.ok_or_else(no_tmux)?);
        let id = t.pane_id(arg)?;
        let p = pm
            .find(&id)
            .ok_or_else(|| Error::Usage(format!("no pane {arg}")))?;
        return Ok(Named::Pane(p.clone()));
    }
    Err(Error::Usage(unknown_seat(ctx, arg, pm)))
}

fn not_an_address(arg: &str) -> Error {
    Error::Usage(format!(
        "'{arg}' is not a seat, sub-seat or pane: write seat, seat@%N, seat/%N or seat/<sub-agent>; no part is empty, '.' or '..' (hail seats lists seats)"
    ))
}

/// Where a message to `arg` goes. `seat/%N` is the pane `%N`, checked to be
/// in that seat; `seat/name` is the parent's mail, marked for the sub-agent.
pub fn mail(ctx: &Ctx, arg: &str, pm: Option<&PaneMap>, tmux: Option<&Tmux>) -> Result<Mail> {
    match Address::parse(arg).ok_or_else(|| not_an_address(arg))? {
        Address::Mailbox(_) => mailbox_mail(ctx, arg, pm, tmux),
        Address::Pane { seat, pane } => {
            let m = mailbox_mail(ctx, &pane, pm, tmux)?;
            if m.to.seat() != seat {
                return Err(Error::Seat(format!(
                    "{arg}: pane {pane} is in seat {}, not {seat}; hail seats lists seats",
                    m.to.seat()
                )));
            }
            Ok(m)
        }
        Address::Agent { parent, name } => {
            let mut m = mailbox_mail(ctx, &parent.to_string(), pm, tmux)?;
            m.for_ = Some(name);
            Ok(m)
        }
    }
}

fn mailbox_mail(ctx: &Ctx, arg: &str, pm: Option<&PaneMap>, tmux: Option<&Tmux>) -> Result<Mail> {
    match name(ctx, arg, pm, tmux)? {
        Named::Sub(to, p) => Ok(sub_seat_mail(ctx, to, p)),
        Named::Seat(seat) => match pm {
            Some(pm) => seat_mail(ctx, pm, seat, None),
            None => Ok(Mail {
                to: Addr::Seat(seat.to_string()),
                for_: None,
                wake: None,
                no_wake: Some("no tmux server".into()),
            }),
        },
        Named::Pane(p) => {
            let pm = pm.ok_or_else(|| Error::State(format!("{arg}: no tmux server answers")))?;
            let seat = pm.seat_of(&p).ok_or_else(|| {
                Error::Seat(format!(
                    "pane {} is not in a seat: {} is not in a jj workspace or git repo",
                    p.id,
                    p.path.display()
                ))
            })?;
            ctx.store.bind_seat(seat, &seat_root(ctx, &p.path, seat))?;
            seat_mail(ctx, pm, seat, Some(&p))
        }
    }
}

/// The pane to read from or type into for `arg`.
pub fn drive(ctx: &Ctx, arg: &str, pm: &PaneMap, tmux: &Tmux) -> Result<Pane> {
    match Address::parse(arg).ok_or_else(|| not_an_address(arg))? {
        Address::Mailbox(_) => {}
        Address::Pane { seat, pane } => {
            let p = drive(ctx, &pane, pm, tmux)?;
            return if pm.seat_of(&p) == Some(seat.as_str()) {
                Ok(p)
            } else {
                Err(Error::Seat(format!(
                    "{arg}: pane {pane} is not in seat {seat}"
                )))
            };
        }
        Address::Agent { parent, .. } => {
            return Err(Error::Usage(format!(
                "{arg} is a sub-agent: it has no pane; its parent {parent} relays to it"
            )));
        }
    }
    match name(ctx, arg, Some(pm), Some(tmux))? {
        Named::Sub(_, p) => Ok(p.clone()),
        Named::Pane(p) => Ok(p),
        Named::Seat(seat) => {
            let agents = pm.agents_in(seat);
            let panes = pm.in_seat(seat);
            match (agents.as_slice(), panes.as_slice()) {
                ([one], _) | ([], [one]) => Ok((*one).clone()),
                (many, _) if many.len() > 1 => Err(shared_error(seat, many)),
                _ => Err(Error::Seat(format!(
                    "seat {seat} has several panes ({}) and no single agent; name one",
                    panes
                        .iter()
                        .map(|p| p.id.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ))),
            }
        }
    }
}

/// Mail to a seat, or to one pane of it: the seat's only agent is woken; in
/// a shared seat a named Claude pane gets its sub-seat, and anything else is
/// refused with the sub-seats listed.
fn seat_mail(ctx: &Ctx, pm: &PaneMap, seat: &str, via: Option<&Pane>) -> Result<Mail> {
    let agents = pm.agents_in(seat);
    if agents.len() > 1 {
        return match via.filter(|p| can_hold_sub_seat(p)) {
            Some(p) => Ok(sub_seat_mail(ctx, Addr::sub(seat, &p.id), p)),
            None => Err(shared_error(seat, &agents)),
        };
    }
    let wake = agents.first().map(|p| (*p).clone());
    let no_wake = wake
        .is_none()
        .then(|| format!("no agent runs in seat {seat}"));
    Ok(Mail {
        to: Addr::Seat(seat.to_string()),
        for_: None,
        wake,
        no_wake,
    })
}

fn sub_seat_mail(ctx: &Ctx, to: Addr, pane: &Pane) -> Mail {
    let _ = std::fs::create_dir_all(ctx.store.seat_dir(&to));
    Mail {
        to,
        for_: None,
        wake: Some(pane.clone()),
        no_wake: None,
    }
}

/// The sending process: its mailboxes, and in a shared seat its own
/// sub-seat, accepted only when `$TMUX_PANE` names a Claude pane in this
/// seat (and the process is not Codex).
/// A Claude pane running hail from a directory that is not its own seat: the
/// send signs as `me`, but the pane's hooks read its own seat, so replies to
/// `me` would never reach it. A warning, never an
/// identity: the working directory still decides who sends. Codex is left
/// out, because its commands carry the shared daemon's `TMUX_PANE`.
pub fn identity_split(ctx: &Ctx, me: &Seat, pm: Option<&PaneMap>) -> Option<String> {
    let pm = pm.filter(|_| !ctx.codex)?;
    let tp = ctx.tmux_pane.as_deref()?;
    let p = pm.find(tp).filter(|p| can_hold_sub_seat(p))?;
    let own = pm.seat_of(p).filter(|s| *s != me.name)?;
    let shared = pm.agents_in(own).len() > 1;
    let address = if shared {
        Addr::sub(own, tp).to_string()
    } else {
        own.to_string()
    };
    Some(format!(
        "this sends as {me} (from this directory), but your pane {tp} works in {path}, seat {address}, where your hooks deliver: replies to {me} will not reach you. Run hail from {path}, or start the session in {root}",
        me = me.name,
        path = p.path.display(),
        root = me.root.display(),
    ))
}

pub fn sender(ctx: &Ctx, me: &Seat, pm: Option<&PaneMap>) -> Sender {
    let mut boxes = ctx.mailboxes(me);
    let Some(pm) = pm else {
        return Sender { boxes, pane: None };
    };
    let agents = pm.agents_in(&me.name);
    if agents.len() > 1 {
        let own = ctx
            .tmux_pane
            .as_deref()
            .filter(|tp| !ctx.codex && agents.iter().any(|p| p.id == *tp && can_hold_sub_seat(p)));
        let Some(tp) = own else {
            return Sender { boxes, pane: None };
        };
        let sub = Addr::sub(&me.name, tp);
        let _ = std::fs::create_dir_all(ctx.store.seat_dir(&sub));
        boxes = Boxes {
            primary: sub,
            seat: Some(Addr::from(me)),
        };
        return Sender {
            boxes,
            pane: Some(tp.to_string()),
        };
    }
    let pane = agents.first().map(|p| p.id.clone()).or_else(|| {
        let tp = ctx.tmux_pane.as_ref()?;
        (pm.seat_of(pm.find(tp)?) == Some(me.name.as_str())).then(|| tp.clone())
    });
    Sender { boxes, pane }
}

fn seat_root(ctx: &Ctx, path: &Path, seat: &str) -> PathBuf {
    seat::seat_of(path, ctx.home.as_deref())
        .ok()
        .flatten()
        .filter(|s| s.name == seat)
        .map_or_else(|| path.to_path_buf(), |s| s.root)
}

fn shared_error(seat: &str, agents: &[&Pane]) -> Error {
    let subs: Vec<String> = agents
        .iter()
        .filter(|p| can_hold_sub_seat(p))
        .map(|p| Addr::sub(seat, &p.id).to_string())
        .collect();
    let mut m = format!(
        "seat {seat} has {} agents; address one: {}",
        agents.len(),
        subs.join(" ")
    );
    if agents.iter().any(|p| !can_hold_sub_seat(p)) {
        m.push_str("; a Codex pane there cannot be addressed apart: give it its own jj workspace");
    }
    Error::Seat(m)
}

fn unknown_seat(ctx: &Ctx, arg: &str, pm: Option<&PaneMap>) -> String {
    let mut names: Vec<String> = ctx
        .store
        .mailboxes()
        .iter()
        .map(|a| a.seat().to_string())
        .collect();
    names.extend(pm.map(PaneMap::seat_names).unwrap_or_default());
    names.sort();
    names.dedup();
    let stem: String = arg.chars().take(3).collect();
    let near: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(&stem))
        .take(8)
        .cloned()
        .collect();
    let near = if near.is_empty() {
        String::new()
    } else {
        format!("; did you mean: {}", near.join(" "))
    };
    // `api-scout`: a seat's name plus a separator is most
    // likely a sub-agent of that seat.
    let parent = names
        .iter()
        .filter(|n| {
            arg.strip_prefix(n.as_str())
                .is_some_and(|rest| rest.len() > 1 && rest.starts_with(['-', '_', '.']))
        })
        .max_by_key(|n| n.len());
    if let Some(p) = parent {
        let child = &arg[p.len() + 1..];
        return format!(
            "'{arg}' is not a seat{near}; if it is a sub-agent of {p}, send to {p}/{child} (its parent relays)"
        );
    }
    format!("unknown seat '{arg}'{near} (hail seats lists them)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_targets() {
        for yes in ["%5", "%123", "sess:1.2", "3"] {
            assert!(looks_like_pane(yes), "{yes}");
        }
        for no in ["%", "%x", "api-1b", ".nix-config", "hail@%28"] {
            assert!(!looks_like_pane(no), "{no}");
        }
    }
}
