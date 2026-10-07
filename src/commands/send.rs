//! `hail <seat> <kind> [headline]` (body on stdin), and the 0.3 form
//! `hail <target> '<headline>' --kind k [--body X]`.

use std::io::Read;
use std::time::Duration;

use crate::bd;
use crate::ctx::{Ctx, PaneMap, looks_like_pane, note_sharing, sub_seat_eligible};
use crate::envelope::{self, Head};
use crate::error::{EXIT_NOT_WOKEN, Error, Result};
use crate::hooks::read_stdin_if_ready;
use crate::seat::{self, Seat};
use crate::store::{ids, message::Message, records::Kind};
use crate::time;
use crate::transport::tmux::{Pane, Tmux};
use crate::transport::{self, TypedEnvelope, Wake, Woken};

#[derive(Debug, Default)]
pub struct SendArgs {
    pub target: String,
    pub kind: String,
    pub headline: Option<String>,
    /// 0.3's `--body`: `-` is stdin, a readable file is read, else literal text.
    pub legacy_body: Option<String>,
    pub legacy: bool,
    pub re: Option<String>,
    pub bead: Option<String>,
    pub scope: Option<String>,
    pub submit: bool,
    pub force: bool,
    pub wake: bool,
}

/// Where a message goes and which pane, if any, to type the envelope into.
pub struct Target {
    pub mailbox: String,
    pub wake: Option<Pane>,
    pub no_wake: Option<String>,
}

pub fn run(ctx: &Ctx, a: SendArgs) -> Result<u8> {
    if !envelope::is_kind(&a.kind) {
        return Err(Error::Usage(format!(
            "unknown kind '{}' ({})",
            a.kind,
            envelope::kinds_list()
        )));
    }
    if matches!(a.kind.as_str(), "done" | "release") && a.re.is_none() {
        return Err(Error::Usage(format!(
            "--kind {} requires --re <id>",
            a.kind
        )));
    }
    if a.re
        .as_deref()
        .is_some_and(|r| r.contains('/') || r.is_empty())
    {
        return Err(Error::Usage("--re must be a message id".into()));
    }
    if a.scope.as_deref().is_some_and(|s| s.contains('\n')) {
        return Err(Error::Usage("--scope must be one line".into()));
    }
    let control = envelope::is_control(&a.kind);

    // Headline and body. A positional headline means stdin is never read.
    let (raw_headline, explicit_body) = headline_and_body(&a)?;
    let mut headline = envelope::sanitize_headline(&raw_headline);
    if headline.is_empty() {
        return Err(Error::Usage(
            "empty headline: say the ask and the why in one line".into(),
        ));
    }
    let mut overflow = None;
    if headline.chars().count() > ctx.max {
        if control {
            return Err(Error::OverCap(format!(
                "headline is {} characters; the cap is {}. A {} is complete in its envelope and has no body; shorten it.",
                headline.chars().count(),
                ctx.max,
                a.kind
            )));
        }
        overflow = Some(headline.clone());
        headline = envelope::fold_headline(&headline, ctx.max);
        eprintln!(
            "hail: headline folded to {} characters (cap {}); the full text rides in the body",
            headline.chars().count(),
            ctx.max
        );
    }
    let has_body = explicit_body.is_some() || overflow.is_some();
    let body = match (&overflow, explicit_body) {
        (Some(full), Some(b)) => format!("{full}\n\n{b}"),
        (Some(full), None) => full.clone(),
        (None, Some(b)) => b,
        (None, None) => headline.clone(),
    };

    // Who and where.
    let me = ctx.require_seat()?;
    let tmux = Tmux::detect().ok();
    let panes = tmux
        .as_ref()
        .and_then(|t| PaneMap::load(t, ctx.home.as_deref()).ok());
    let (my_box, my_pane) = sender(ctx, &me, panes.as_ref());
    let target = resolve(ctx, &a.target, panes.as_ref(), tmux.as_ref())?;

    // State preconditions, checked before anything is written or typed.
    if a.kind == "release"
        && !ctx
            .store
            .has_record(Kind::Holds, "", a.re.as_deref().unwrap_or(""))
    {
        return Err(Error::State(format!(
            "no hold or block in effect with id {}; hail brief lists holds",
            a.re.as_deref().unwrap_or("")
        )));
    }
    // An obligation may sit on this pane's sub-seat or on the seat.
    let mut my_boxes = ctx.my_mailboxes(&me);
    if !my_boxes.contains(&my_box) {
        my_boxes.insert(0, my_box.clone());
    }
    if a.kind == "done"
        && !my_boxes.iter().any(|b| {
            ctx.store
                .has_record(Kind::Owed, b, a.re.as_deref().unwrap_or(""))
        })
    {
        return Err(Error::State(format!(
            "no open obligation {} on {my_box}; hail brief lists yours",
            a.re.as_deref().unwrap_or("")
        )));
    }
    let wake_pane = if a.wake { target.wake.clone() } else { None };
    if let (Some(t), Some(p), false) = (&tmux, &wake_pane, a.force) {
        transport::guard_dialog(t, p)?;
    }

    // The message is durable before anything is typed.
    let now = time::now();
    let id = ids::reserve(&ctx.store, &target.mailbox, now)?;
    let bead = a.bead.clone().or_else(|| envelope::detect_bead(&headline));
    let from = match &my_pane {
        Some(p) if !my_box.contains('@') => format!("{my_box}/{p}"),
        _ => my_box.clone(),
    };
    let msg = Message::default()
        .header("from", &from)
        .header("reply", &my_box)
        .header("kind", &a.kind)
        .header("id", &id)
        .header("time", time::iso(now))
        .header_opt("bead", bead.as_deref())
        .header_opt("re", a.re.as_deref())
        .header_opt("scope", a.scope.as_deref())
        .header("ask", &headline);
    let msg = Message {
        body: body.clone(),
        ..msg
    };
    ctx.store
        .mailbox(&target.mailbox)
        .post(&id, &msg.render(), control)?;
    record_state(
        ctx,
        &a,
        &id,
        &my_box,
        &my_boxes,
        &target.mailbox,
        &headline,
        now,
    )?;

    // The envelope.
    let head = Head {
        kind: &a.kind,
        from: &from,
        reply: &my_box,
        id: &id,
        bead: bead.as_deref(),
        re: a.re.as_deref(),
        scope: a.scope.as_deref(),
    };
    let env = envelope::render(&head, &headline, has_body && !control);
    let not_woken = match (&tmux, &wake_pane) {
        (Some(t), Some(p)) => match (TypedEnvelope { tmux: t }).wake(p, &env, a.submit) {
            Ok(Woken::Typed) => None,
            Ok(Woken::NotConfirmed) => Some(format!(
                "typed into {} but not seen there after 2 s, so not submitted; hail read {} 10 to check, then hail keys {} Enter",
                p.id, p.id, p.id
            )),
            Err(e) => Some(format!("typing into {} failed: {e}", p.id)),
        },
        _ if !a.wake => Some("--no-wake".into()),
        _ => Some(
            target
                .no_wake
                .clone()
                .unwrap_or_else(|| "no pane to wake".into()),
        ),
    };

    outln!("id={id}");
    if let Some(b) = &bead {
        match bd::comment(b, &body) {
            Some(Some(n)) => outln!("bead={b} comment={n}"),
            Some(None) => {}
            None => eprintln!(
                "hail: warning: could not post to bead {b} with bd; the message is file-only"
            ),
        }
    }
    match not_woken {
        None => Ok(0),
        Some(reason) => {
            eprintln!(
                "hail: delivered to {}'s inbox; not typed ({reason}); do not resend: it arrives on their next prompt",
                target.mailbox
            );
            Ok(EXIT_NOT_WOKEN)
        }
    }
}

fn headline_and_body(a: &SendArgs) -> Result<(String, Option<String>)> {
    if a.legacy {
        let headline = a.headline.clone().unwrap_or_default();
        let body = match a.legacy_body.as_deref() {
            None => None,
            Some("-") => {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| Error::State(format!("reading stdin: {e}")))?;
                Some(s.trim_end_matches('\n').to_string())
            }
            Some(src) => match std::fs::read_to_string(src) {
                Ok(s) if std::path::Path::new(src).is_file() => {
                    Some(s.trim_end_matches('\n').to_string())
                }
                _ => Some(src.to_string()),
            },
        };
        return Ok((headline, body));
    }
    if let Some(h) = &a.headline {
        // A heredoc or file on stdin is readable at once; an idle open pipe
        // is not, and is never waited on. Data there is the body: dropping it
        // would lose what the sender wrote.
        let body = read_stdin_if_ready(Duration::from_millis(10))
            .map(|b| String::from_utf8_lossy(&b).trim_matches('\n').to_string())
            .filter(|b| !b.trim().is_empty());
        return Ok((h.clone(), body));
    }
    // The headline must come from stdin here, so waiting costs nothing on success:
    // a slow producer (`make-report | hail …`) still gets through.
    let Some(bytes) = read_stdin_if_ready(Duration::from_secs(2)) else {
        return Err(Error::Usage(
            "no headline: pass it as an argument, or the headline then the body on stdin (heredoc)"
                .into(),
        ));
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let rest = rest
        .strip_prefix('\n')
        .unwrap_or(rest)
        .trim_end_matches('\n');
    Ok((
        first.to_string(),
        (!rest.is_empty()).then(|| rest.to_string()),
    ))
}

/// The sender's mailbox and pane. In a shared seat a Claude sender is its
/// sub-seat, accepted only when `$TMUX_PANE` names a Claude pane in this seat.
fn sender(ctx: &Ctx, me: &Seat, panes: Option<&PaneMap>) -> (String, Option<String>) {
    let Some(pm) = panes else {
        return (ctx.my_mailbox(me), None);
    };
    let agents = pm.agents_in(&me.name);
    note_sharing(&ctx.store, &me.name, &agents);
    if agents.len() > 1 {
        if let (Some(tp), false) = (&ctx.tmux_pane, ctx.is_codex()) {
            if agents.iter().any(|p| &p.id == tp && sub_seat_eligible(p)) {
                let sub = seat::sub_seat(&me.name, tp);
                let _ = std::fs::create_dir_all(ctx.store.seat_dir(&sub));
                return (sub, Some(tp.clone()));
            }
        }
        return (me.name.clone(), None);
    }
    let pane = agents.first().map(|p| p.id.clone()).or_else(|| {
        let tp = ctx.tmux_pane.as_ref()?;
        let p = pm.find(tp)?;
        (pm.seat_of(p) == Some(me.name.as_str())).then(|| tp.clone())
    });
    (me.name.clone(), pane)
}

/// Resolve a target: a sub-seat (`hail@%28`), a seat, or a pane.
pub fn resolve(
    ctx: &Ctx,
    arg: &str,
    panes: Option<&PaneMap>,
    tmux: Option<&Tmux>,
) -> Result<Target> {
    let no_tmux = || {
        Error::State(format!(
            "{arg}: no tmux server answers; hail doctor says why"
        ))
    };
    if let (base, Some(pid)) = seat::split_sub_seat(arg) {
        let pm = panes.ok_or_else(no_tmux)?;
        let p = pm.find(pid).ok_or_else(|| {
            Error::Seat(format!(
                "{arg}: pane {pid} is gone; hail seats lists live seats"
            ))
        })?;
        if pm.seat_of(p) != Some(base) || !sub_seat_eligible(p) {
            return Err(Error::Seat(format!(
                "{arg}: {pid} is not a Claude pane in seat {base}; hail seats lists seats"
            )));
        }
        let _ = std::fs::create_dir_all(ctx.store.seat_dir(arg));
        return Ok(Target {
            mailbox: arg.to_string(),
            wake: Some(p.clone()),
            no_wake: None,
        });
    }
    // A seat is known once it has a mailbox (an agent ran hail there, or mail
    // was migrated to it), or while a pane sits in it.
    let known =
        ctx.store.seat_dir(arg).is_dir() || panes.is_some_and(|pm| !pm.in_seat(arg).is_empty());
    if !known && looks_like_pane(arg) {
        let (pm, t) = (panes.ok_or_else(no_tmux)?, tmux.ok_or_else(no_tmux)?);
        let id = t.pane_id(arg)?;
        let p = pm
            .find(&id)
            .ok_or_else(|| Error::Usage(format!("no pane {arg}")))?;
        let seat = pm.seat_of(p).ok_or_else(|| {
            Error::Seat(format!(
                "pane {id} is not in a seat: {} is not in a jj workspace or git repo",
                p.path.display()
            ))
        })?;
        let agents = pm.agents_in(seat);
        note_sharing(&ctx.store, seat, &agents);
        ctx.store.bind_seat(seat, &seat_root(ctx, &p.path, seat))?;
        if agents.len() > 1 {
            if sub_seat_eligible(p) {
                let sub = seat::sub_seat(seat, &id);
                let _ = std::fs::create_dir_all(ctx.store.seat_dir(&sub));
                return Ok(Target {
                    mailbox: sub,
                    wake: Some(p.clone()),
                    no_wake: None,
                });
            }
            return Err(shared_error(seat, &agents));
        }
        let wake = agents.first().map(|p| (*p).clone());
        let no_wake = wake
            .is_none()
            .then(|| format!("no agent runs in seat {seat}"));
        return Ok(Target {
            mailbox: seat.to_string(),
            wake,
            no_wake,
        });
    }
    if !known {
        return Err(Error::Usage(unknown_seat(ctx, arg, panes)));
    }
    let Some(pm) = panes else {
        return Ok(Target {
            mailbox: arg.to_string(),
            wake: None,
            no_wake: Some("no tmux server".into()),
        });
    };
    let agents = pm.agents_in(arg);
    note_sharing(&ctx.store, arg, &agents);
    match agents.len() {
        0 => Ok(Target {
            mailbox: arg.to_string(),
            wake: None,
            no_wake: Some(format!("no agent runs in seat {arg}")),
        }),
        1 => Ok(Target {
            mailbox: arg.to_string(),
            wake: Some(agents[0].clone()),
            no_wake: None,
        }),
        _ => Err(shared_error(arg, &agents)),
    }
}

fn seat_root(ctx: &Ctx, path: &std::path::Path, seat: &str) -> std::path::PathBuf {
    crate::seat::seat_of(path, ctx.home.as_deref())
        .ok()
        .flatten()
        .filter(|s| s.name == seat)
        .map(|s| s.root)
        .unwrap_or_else(|| path.to_path_buf())
}

fn shared_error(seat: &str, agents: &[&Pane]) -> Error {
    let subs: Vec<String> = agents
        .iter()
        .filter(|p| sub_seat_eligible(p))
        .map(|p| seat::sub_seat(seat, &p.id))
        .collect();
    let codex = agents.iter().any(|p| !sub_seat_eligible(p));
    let mut m = format!(
        "seat {seat} has {} agents; address one: {}",
        agents.len(),
        subs.join(" ")
    );
    if codex {
        m.push_str("; a Codex pane there cannot be addressed apart: give it its own jj workspace");
    }
    Error::Seat(m)
}

fn unknown_seat(ctx: &Ctx, arg: &str, panes: Option<&PaneMap>) -> String {
    let mut names = ctx.store.seat_names();
    if let Some(pm) = panes {
        names.extend(pm.seat_names());
    }
    names.sort();
    names.dedup();
    let stem: String = arg.chars().take(3).collect();
    let near: Vec<&String> = names
        .iter()
        .filter(|n| !n.contains('@') && n.starts_with(&stem))
        .take(8)
        .collect();
    let near = if near.is_empty() {
        String::new()
    } else {
        format!(
            "; did you mean: {}",
            near.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    format!("unknown seat '{arg}'{near} (hail seats lists them)")
}

#[allow(clippy::too_many_arguments)]
fn record_state(
    ctx: &Ctx,
    a: &SendArgs,
    id: &str,
    me: &str,
    my_boxes: &[String],
    to: &str,
    headline: &str,
    now: jiff::Timestamp,
) -> Result<()> {
    let rec = || {
        Message::default()
            .header("id", id)
            .header("kind", &a.kind)
            .header("issuer", me)
            .header("to", to)
            .header("scope", a.scope.as_deref().unwrap_or(""))
            .header("time", time::iso(now))
            .header_opt("re", a.re.as_deref())
            .header("headline", headline)
    };
    let re = a.re.as_deref().unwrap_or("");
    match a.kind.as_str() {
        "hold" | "block" => ctx.store.put_record(Kind::Holds, "", id, &rec())?,
        "release" => ctx.store.remove_record(Kind::Holds, "", re)?,
        k if envelope::creates_obligation(k) => ctx.store.put_record(Kind::Owed, to, id, &rec())?,
        "done" => {
            for b in my_boxes {
                ctx.store.remove_record(Kind::Owed, b, re)?;
            }
        }
        _ => {}
    }
    if !envelope::is_control(&a.kind) {
        let pending = Message::default()
            .header("id", id)
            .header("kind", &a.kind)
            .header("to", to)
            .header("epoch", now.as_second().to_string())
            .header("headline", headline);
        ctx.store.put_record(Kind::Pending, me, id, &pending)?;
    }
    Ok(())
}
