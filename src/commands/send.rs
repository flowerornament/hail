//! `hail <seat> <kind> [headline]`, the body on stdin.
//!
//! A send is: compose the message, find who is sending and where it goes
//! (`route`), check the state it changes, decide whether to type, write it
//! durably, wake the recipient, and tell the sender what became of it
//! (`Outcome::tell`).

use jiff::Timestamp;

use crate::ctx::Ctx;
use crate::envelope::{self, Head, Kind};
use crate::error::{EXIT_NOT_WOKEN, Error, Result};
use crate::input;
use crate::policy::QUIET_HOOKS_SEEN;
use crate::route::{self, Mail, Sender};
use crate::seat::Addr;
use crate::store::ids::{self, Id};
use crate::store::message::Message;
use crate::store::records::{Entry, Pending, hold_default};
use crate::time;
use crate::transport::pane_map::PaneMap;
use crate::transport::tmux::{Pane, Tmux};
use crate::transport::{self, TypedEnvelope, Wake, Woken};

#[derive(Debug)]
pub struct SendArgs {
    pub target: String,
    pub kind: Kind,
    /// The headline as an argument; without one, the first line of stdin
    /// is the headline and the rest the body.
    pub headline: Option<String>,
    /// The message this answers, or lifts (`release`).
    pub re: Option<Id>,
    pub scope: Option<String>,
    /// `--as <name>`: a sub-agent signs as `<mailbox>/<name>`, so replies
    /// come back to its parent's mailbox marked for it.
    pub as_name: Option<String>,
    /// `--for`: when a hold or block lapses; its kind's default otherwise.
    pub hold_for: Option<std::time::Duration>,
    pub delivery: Delivery,
}

/// How far the transport goes.
#[derive(Debug, Clone, Copy)]
pub struct Delivery {
    /// Type the envelope at all (`--no-wake` turns it off).
    pub wake: bool,
    /// Press Enter after it (`--no-submit` turns it off).
    pub submit: bool,
    /// Skip the dialog guard (`--force`).
    pub force: bool,
}

/// The message as the recipient will see it.
struct Draft {
    headline: String,
    body: String,
    /// The envelope carries the `— hail inbox` fetch hint.
    has_body: bool,
}

/// Who is sending, where it goes, and the tmux that can wake it.
struct Route {
    tmux: Option<Tmux>,
    sender: Sender,
    mail: Mail,
}

/// Whether the send types anything, decided before anything is written.
struct Plan<'a> {
    /// A hook has read the recipient's mailbox lately.
    hooked: bool,
    /// An fyi to a hooked seat: written, never typed.
    quiet: bool,
    /// The pane the envelope is typed into.
    pane: Option<&'a Pane>,
}

/// The message as written: its id, and the envelope to type.
struct Written {
    id: Id,
    envelope: String,
}

/// What became of the envelope after the message was written.
#[derive(Debug)]
enum Outcome {
    /// A quiet fyi: nothing typed.
    Quiet,
    /// Nothing typed, and why.
    NotTyped(String),
    /// Typed into `pane`, and what the transport saw there.
    Typed { pane: String, woken: Woken },
    /// Typing into `pane` failed.
    Failed { pane: String, error: String },
}

pub fn run(ctx: &Ctx, a: &SendArgs) -> Result<u8> {
    validate(a)?;
    let draft = compose(ctx, a)?;
    let r = route(ctx, a)?;
    check_state(ctx, a)?;
    let plan = decide(ctx, a, &r)?;
    let w = write(ctx, a, &r, &draft, plan.quiet)?;
    let outcome = if plan.quiet {
        Outcome::Quiet
    } else {
        wake(r.tmux.as_ref(), plan.pane, &w.envelope, a.delivery, &r.mail)
    };
    let unread_where = (!plan.hooked).then(|| {
        ctx.store
            .seat_root(r.mail.to.seat())
            .map_or_else(|| "its seat".into(), |p| p.display().to_string())
    });
    outln!("id={}", w.id);
    let (line, code) = outcome.tell(&r.mail.to, unread_where.as_deref());
    if let Some(line) = line {
        eprintln!("hail: {line}");
    }
    Ok(code)
}

/// Find who is sending and where the message goes.
fn route(ctx: &Ctx, a: &SendArgs) -> Result<Route> {
    let me = ctx.require_seat()?;
    let tmux = Tmux::detect().ok();
    let panes = tmux
        .as_ref()
        .and_then(|t| PaneMap::load(t, ctx.home.as_deref()).ok());
    let sender = route::sender(ctx, &me, panes.as_ref());
    if let Some(w) = route::identity_split(ctx, &me, panes.as_ref()) {
        eprintln!("hail: warning: {w}");
    }
    let mail = route::mail(ctx, &a.target, panes.as_ref(), tmux.as_ref())?;
    Ok(Route { tmux, sender, mail })
}

/// Decide whether to type the envelope, and refuse a pane showing a dialog
/// before anything is written.
fn decide<'a>(ctx: &Ctx, a: &SendArgs, r: &'a Route) -> Result<Plan<'a>> {
    // An fyi never interrupts a seat whose hooks run: it rides in with the
    // recipient's next prompt. Where hooks have not run, typing is the only
    // way it would be seen, so it is typed as before. An fyi that answers a
    // question is typed too: the asker is waiting for it.
    let hooked = ctx
        .store
        .mailbox(&r.mail.to)
        .hooked_within(QUIET_HOOKS_SEEN);
    let quiet = a.kind == Kind::Fyi && hooked && !answers_a_question(ctx, a, &r.sender);
    let pane = r.mail.wake.as_ref().filter(|_| a.delivery.wake && !quiet);
    if let (Some(t), Some(p), false) = (&r.tmux, pane, a.delivery.force) {
        transport::guard_dialog(t, p)?;
    }
    Ok(Plan {
        hooked,
        quiet,
        pane,
    })
}

/// `--re` names a ruling, go or ask that the sender received.
fn answers_a_question(ctx: &Ctx, a: &SendArgs, sender: &Sender) -> bool {
    let Some(re) = &a.re else {
        return false;
    };
    sender
        .boxes
        .iter()
        .find_map(|b| ctx.store.mailbox(b).find(re))
        .and_then(|f| std::fs::read_to_string(f.path).ok())
        .and_then(|text| Kind::parse(Message::parse(&text).get("kind")?))
        .is_some_and(Kind::asks_for_reply)
}

/// Write the message durably and record the state it changes, before
/// anything is typed; return the envelope to type.
fn write(ctx: &Ctx, a: &SendArgs, r: &Route, draft: &Draft, quiet: bool) -> Result<Written> {
    let (sender, mail) = (&r.sender, &r.mail);
    let now = time::now();
    let id = ids::reserve(&ctx.store, &mail.to, now)?;
    let (from, reply) = match &a.as_name {
        Some(name) => {
            let me = format!("{}/{name}", sender.boxes.primary);
            (me.clone(), me)
        }
        None => (sender.from(), sender.boxes.primary.to_string()),
    };
    let until = a.kind.is_hold().then(|| time::iso(expires_at(a, now)));
    let head = Head {
        kind: a.kind,
        from: &from,
        reply: &reply,
        id: id.as_str(),
        for_: mail.for_.as_deref(),
        re: a.re.as_ref().map(Id::as_str),
        scope: a.scope.as_deref(),
        until: until.as_deref(),
    };
    let msg = head.message(&time::iso(now), &draft.headline, &draft.body);
    ctx.store
        .mailbox(&mail.to)
        .post(&id, &msg.render(), a.kind.is_control())?;
    record_state(ctx, a, &id, sender, mail, &draft.headline, now)?;
    // A quiet fyi is received whenever its reader next works: a missing
    // receipt after two minutes would be noise in the sender's brief.
    if !a.kind.is_control() && !quiet {
        let pending = Pending {
            id: id.clone(),
            kind: Some(a.kind),
            to: mail.to.clone(),
            sent: Some(now),
            headline: draft.headline.clone(),
        };
        ctx.store.put_pending(&sender.boxes.primary, &pending)?;
    }
    let envelope = envelope::render(
        &head,
        &draft.headline,
        draft.has_body && !a.kind.is_control(),
    );
    Ok(Written { id, envelope })
}

/// Type the envelope into the wake pane, when there is one.
fn wake(
    tmux: Option<&Tmux>,
    pane: Option<&Pane>,
    envelope: &str,
    d: Delivery,
    mail: &Mail,
) -> Outcome {
    let (Some(t), Some(p)) = (tmux, pane) else {
        return Outcome::NotTyped(if d.wake {
            mail.no_wake
                .clone()
                .unwrap_or_else(|| "no pane to wake".into())
        } else {
            "--no-wake".into()
        });
    };
    let pane = p.id.clone();
    match (TypedEnvelope { tmux: t }).wake(p, envelope, d.submit) {
        Ok(woken) => Outcome::Typed { pane, woken },
        Err(e) => Outcome::Failed {
            pane,
            error: e.to_string(),
        },
    }
}

impl Outcome {
    /// What the sender is told on stderr after the `id=` line, and the exit
    /// code: 5 when the recipient's agent has not been given the message.
    /// `unread_where` is set when no hook has read the mailbox lately: then
    /// nothing will deliver it until someone runs `hail inbox` there.
    fn tell(&self, to: &Addr, unread_where: Option<&str>) -> (Option<String>, u8) {
        let Some(reason) = self.not_woken() else {
            let note = match self {
                Self::Quiet => Some(format!(
                    "quiet: arrives with {to}'s next prompt (an fyi never interrupts; an ask or announce does)"
                )),
                Self::Typed {
                    pane,
                    woken: Woken::Steer,
                } => Some(format!(
                    "Codex in {pane} took it as a steer: the agent gets it at its next tool call"
                )),
                _ => None,
            };
            return (note, 0);
        };
        let line = match unread_where {
            None => format!(
                "delivered to {to}'s inbox; not submitted ({reason}); do not resend: it arrives on their next prompt"
            ),
            Some(root) => format!(
                "delivered to {to}'s inbox; not submitted ({reason}). No hook has read {to} lately, so it waits until someone runs hail inbox in {root}. If its agent works from another directory, hail seats shows where; do not resend."
            ),
        };
        (Some(line), EXIT_NOT_WOKEN)
    }

    /// Why the agent has not been given the message, when it has not.
    fn not_woken(&self) -> Option<String> {
        let (pane, woken) = match self {
            Self::Quiet => return None,
            Self::NotTyped(reason) => return Some(reason.clone()),
            Self::Failed { pane, error } => {
                return Some(format!("typing into {pane} failed: {error}"));
            }
            Self::Typed { pane, woken } => (pane, woken),
        };
        match woken {
            Woken::Typed | Woken::Steer => None,
            Woken::Held { queued, goal } => Some(format!(
                "typed into {pane}, but Codex holds it {}{}. The agent has not seen it; to send it now, press Esc in {pane}, which interrupts its turn",
                if *queued {
                    "until no turn is running"
                } else {
                    "until its turn ends"
                },
                if *goal {
                    ", and while it pursues a goal turns follow each other, so that may be long"
                } else {
                    ""
                }
            )),
            Woken::NotSubmitted => Some(format!(
                "typed into {pane}, but it is still in the composer after Enter, twice: press Enter in {pane}"
            )),
            Woken::NotConfirmed => Some(format!(
                "typed into {pane} but not seen there after 10 s, so not submitted; hail read {pane} 10 to check, then hail keys {pane} Enter"
            )),
        }
    }
}

fn validate(a: &SendArgs) -> Result<()> {
    if a.kind.needs_re() && a.re.is_none() {
        return Err(Error::Usage(format!("a {} requires --re <id>", a.kind)));
    }
    if a.scope.as_deref().is_some_and(|s| s.contains('\n')) {
        return Err(Error::Usage("--scope must be one line".into()));
    }
    Ok(())
}

/// The headline (sanitized, folded over the cap) and the body.
fn compose(ctx: &Ctx, a: &SendArgs) -> Result<Draft> {
    let (raw, body) = headline_and_body(a.headline.as_deref())?;
    let headline = envelope::sanitize_headline(&raw);
    if headline.is_empty() {
        return Err(Error::Usage(
            "empty headline: say the ask and the why in one line".into(),
        ));
    }
    let chars = headline.chars().count();
    if chars <= ctx.max {
        let has_body = body.is_some();
        let body = body.unwrap_or_else(|| headline.clone());
        return Ok(Draft {
            headline,
            body,
            has_body,
        });
    }
    if a.kind.is_control() {
        return Err(Error::OverCap(format!(
            "headline is {chars} characters; the cap is {}. A {} is complete in its envelope and has no body; shorten it.",
            ctx.max, a.kind
        )));
    }
    // The sender wrote one message; the tool decides the envelope.
    let folded = envelope::fold_headline(&headline, ctx.max);
    eprintln!(
        "hail: headline folded to {} characters (cap {}); the full text rides in the body",
        folded.chars().count(),
        ctx.max
    );
    let body = match body {
        Some(b) => format!("{headline}\n\n{b}"),
        None => headline,
    };
    Ok(Draft {
        headline: folded,
        body,
        has_body: true,
    })
}

pub fn headline_and_body(headline: Option<&str>) -> Result<(String, Option<String>)> {
    if let Some(h) = headline {
        // Whatever stdin holds is the body: dropping it would lose what the
        // sender wrote.
        let body = input::read()?
            .map(|b| String::from_utf8_lossy(&b).trim_matches('\n').to_string())
            .filter(|b| !b.trim().is_empty());
        return Ok((h.to_string(), body));
    }
    let Some(bytes) = input::read()? else {
        return Err(Error::Usage(
            "no headline: pass it as an argument, or the headline then the body on stdin (heredoc)"
                .into(),
        ));
    };
    let text = String::from_utf8_lossy(&bytes);
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

/// A release must refer to a real hold before anything is written or typed.
fn check_state(ctx: &Ctx, a: &SendArgs) -> Result<()> {
    match (a.kind, &a.re) {
        (Kind::Release, Some(re)) => match ctx.store.hold(re) {
            None => Err(Error::State(format!(
                "no hold or block with id {re}; hail brief lists holds in effect"
            ))),
            Some(h) => {
                let at = h.lapses_at();
                if at <= time::now() {
                    eprintln!(
                        "hail: {re} had already lapsed at {}; releasing it anyway tells its recipient",
                        time::iso(at)
                    );
                }
                Ok(())
            }
        },
        _ => Ok(()),
    }
}

/// When a hold or block sent now lapses.
fn expires_at(a: &SendArgs, now: Timestamp) -> Timestamp {
    time::after(now, a.hold_for.unwrap_or_else(|| hold_default(a.kind)))
}

fn record_state(
    ctx: &Ctx,
    a: &SendArgs,
    id: &Id,
    sender: &Sender,
    mail: &Mail,
    headline: &str,
    now: Timestamp,
) -> Result<()> {
    let entry = || Entry {
        id: id.clone(),
        kind: Some(a.kind),
        issuer: sender.boxes.primary.clone(),
        to: mail.to.clone(),
        scope: a.scope.clone(),
        time: Some(now),
        re: a.re.clone(),
        expires: Some(expires_at(a, now)),
        headline: headline.to_string(),
    };
    match (a.kind, &a.re) {
        (k, _) if k.is_hold() => ctx.store.put_hold(&entry())?,
        (Kind::Release, Some(re)) => ctx.store.remove_hold(re)?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tell(o: &Outcome, unread_where: Option<&str>) -> (Option<String>, u8) {
        o.tell(&Addr::parse("web-1b").expect("an address"), unread_where)
    }

    fn typed(woken: Woken) -> Outcome {
        Outcome::Typed {
            pane: "%3".into(),
            woken,
        }
    }

    #[test]
    fn woken_is_exit_0() {
        assert_eq!(tell(&typed(Woken::Typed), None), (None, 0));
        let (steer, code) = tell(&typed(Woken::Steer), Some("/x"));
        assert_eq!(code, 0);
        assert_eq!(
            steer.as_deref(),
            Some("Codex in %3 took it as a steer: the agent gets it at its next tool call")
        );
        let (quiet, code) = tell(&Outcome::Quiet, None);
        assert_eq!(code, 0);
        assert!(quiet.is_some_and(|q| q.starts_with("quiet: arrives with web-1b's next prompt")));
    }

    #[test]
    fn not_woken_is_exit_5_with_the_reason() {
        let held = typed(Woken::Held {
            queued: true,
            goal: true,
        });
        let (line, code) = tell(&held, None);
        assert_eq!(code, EXIT_NOT_WOKEN);
        assert_eq!(
            line.as_deref(),
            Some(
                "delivered to web-1b's inbox; not submitted (typed into %3, but Codex holds it until no turn is running, and while it pursues a goal turns follow each other, so that may be long. The agent has not seen it; to send it now, press Esc in %3, which interrupts its turn); do not resend: it arrives on their next prompt"
            )
        );
        let (line, code) = tell(&Outcome::NotTyped("--no-wake".into()), Some("/w"));
        assert_eq!(code, EXIT_NOT_WOKEN);
        assert!(line.is_some_and(|l| l.starts_with(
            "delivered to web-1b's inbox; not submitted (--no-wake). No hook has read web-1b lately, so it waits until someone runs hail inbox in /w."
        )));
        let failed = Outcome::Failed {
            pane: "%3".into(),
            error: "boom".into(),
        };
        assert_eq!(
            failed.not_woken().as_deref(),
            Some("typing into %3 failed: boom")
        );
    }
}
