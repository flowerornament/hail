//! `hail <seat> <kind> [headline]` (body on stdin), and the 0.3 form
//! `hail <target> '<headline>' --kind k [--body X]`.
//!
//! A send is: compose the message, find who is sending and where it goes
//! (`route`), check the state it changes, write it durably, then wake the
//! recipient.

use std::io::Read;
use std::path::Path;

use jiff::Timestamp;

use crate::bd::{self, Posted};
use crate::ctx::Ctx;
use crate::envelope::{self, Head, Kind};
use crate::error::{EXIT_NOT_WOKEN, Error, Result};
use crate::input;
use crate::route::{self, Mail, Sender};
use crate::store::ids::{self, Id};
use crate::store::message::Message;
use crate::store::records::{Entry, Pending};
use crate::time;
use crate::transport::panes::PaneMap;
use crate::transport::tmux::{Pane, Tmux};
use crate::transport::{self, TypedEnvelope, Wake, Woken};

#[derive(Debug)]
pub struct SendArgs {
    pub target: String,
    pub kind: Kind,
    pub form: Form,
    /// The message this answers, closes (`done`) or lifts (`release`).
    pub re: Option<Id>,
    pub bead: Option<String>,
    pub scope: Option<String>,
    pub delivery: Delivery,
}

/// Where the headline and body come from.
#[derive(Debug)]
pub enum Form {
    /// The headline as an argument, or as the first line of stdin with the
    /// body after it.
    Current { headline: Option<String> },
    /// 0.3: the headline as an argument, and `--body`: `-` is stdin, a
    /// readable file is read, anything else is literal text. Removed in 0.5.
    Legacy {
        headline: String,
        body: Option<String>,
    },
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

pub fn run(ctx: &Ctx, a: &SendArgs) -> Result<u8> {
    validate(a)?;
    let draft = compose(ctx, a)?;
    let me = ctx.require_seat()?;
    let tmux = Tmux::detect().ok();
    let panes = tmux
        .as_ref()
        .and_then(|t| PaneMap::load(t, ctx.home.as_deref()).ok());
    let sender = route::sender(ctx, &me, panes.as_ref());
    let mail = route::mail(ctx, &a.target, panes.as_ref(), tmux.as_ref())?;
    check_state(ctx, a, &sender)?;
    let wake_pane = mail.wake.as_ref().filter(|_| a.delivery.wake);
    if let (Some(t), Some(p), false) = (&tmux, wake_pane, a.delivery.force) {
        transport::guard_dialog(t, p)?;
    }

    // The message is durable before anything is typed.
    let now = time::now();
    let id = ids::reserve(&ctx.store, &mail.to, now)?;
    let bead = a
        .bead
        .clone()
        .or_else(|| envelope::detect_bead(&draft.headline));
    let from = sender.from();
    let reply = sender.boxes.primary.to_string();
    let re = a.re.as_ref().map(Id::as_str);
    let msg = Message::default()
        .header("from", &from)
        .header("reply", &reply)
        .header("kind", a.kind.as_str())
        .header("id", id.as_str())
        .header("time", time::iso(now))
        .header_opt("bead", bead.as_deref())
        .header_opt("re", re)
        .header_opt("scope", a.scope.as_deref())
        .header("ask", &draft.headline)
        .with_body(&draft.body);
    ctx.store
        .mailbox(&mail.to)
        .post(&id, &msg.render(), a.kind.is_control())?;
    record_state(ctx, a, &id, &sender, &mail, &draft.headline, now)?;

    let head = Head {
        kind: a.kind,
        from: &from,
        reply: &reply,
        id: id.as_str(),
        bead: bead.as_deref(),
        re,
        scope: a.scope.as_deref(),
    };
    let envelope = envelope::render(
        &head,
        &draft.headline,
        draft.has_body && !a.kind.is_control(),
    );
    let not_woken = wake(tmux.as_ref(), wake_pane, &envelope, a.delivery, &mail);

    outln!("id={id}");
    if let Some(b) = &bead {
        match bd::comment(b, &draft.body) {
            Posted::Comment(n) => outln!("bead={b} comment={n}"),
            Posted::Unnumbered => {}
            Posted::Failed => {
                eprintln!(
                    "hail: warning: could not post to bead {b} with bd; the message is file-only"
                );
            }
        }
    }
    let Some(reason) = not_woken else {
        return Ok(0);
    };
    eprintln!(
        "hail: delivered to {}'s inbox; not typed ({reason}); do not resend: it arrives on their next prompt",
        mail.to
    );
    Ok(EXIT_NOT_WOKEN)
}

fn validate(a: &SendArgs) -> Result<()> {
    if a.kind.needs_re() && a.re.is_none() {
        return Err(Error::Usage(format!(
            "--kind {} requires --re <id>",
            a.kind
        )));
    }
    if a.scope.as_deref().is_some_and(|s| s.contains('\n')) {
        return Err(Error::Usage("--scope must be one line".into()));
    }
    Ok(())
}

/// The headline (sanitized, folded over the cap) and the body.
fn compose(ctx: &Ctx, a: &SendArgs) -> Result<Draft> {
    let (raw, body) = headline_and_body(&a.form)?;
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

fn headline_and_body(form: &Form) -> Result<(String, Option<String>)> {
    match form {
        Form::Legacy { headline, body } => Ok((
            headline.clone(),
            body.as_deref().map(legacy_body).transpose()?,
        )),
        Form::Current { headline: Some(h) } => {
            // Whatever stdin holds is the body: dropping it would lose what
            // the sender wrote.
            let body = input::read()?
                .map(|b| String::from_utf8_lossy(&b).trim_matches('\n').to_string())
                .filter(|b| !b.trim().is_empty());
            Ok((h.clone(), body))
        }
        Form::Current { headline: None } => {
            let Some(bytes) = input::read()? else {
                return Err(Error::Usage(
                    "no headline: pass it as an argument, or the headline then the body on stdin (heredoc)".into(),
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
    }
}

/// 0.3's `--body`: `-` is stdin, an existing file is read, else literal text.
fn legacy_body(src: &str) -> Result<String> {
    if src == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Error::State(format!("reading stdin: {e}")))?;
        return Ok(s.trim_end_matches('\n').to_string());
    }
    let path = Path::new(src);
    Ok(match std::fs::read_to_string(path) {
        Ok(s) if path.is_file() => s.trim_end_matches('\n').to_string(),
        _ => src.to_string(),
    })
}

/// Releases and `done` must refer to something real before anything is
/// written or typed.
fn check_state(ctx: &Ctx, a: &SendArgs, sender: &Sender) -> Result<()> {
    match (a.kind, &a.re) {
        (Kind::Release, Some(re)) if !ctx.store.has_hold(re) => Err(Error::State(format!(
            "no hold or block in effect with id {re}; hail brief lists holds"
        ))),
        (Kind::Done, Some(re)) if !sender.boxes.iter().any(|b| ctx.store.has_owed(b, re)) => {
            Err(Error::State(format!(
                "no open obligation {re} on {}; hail brief lists yours",
                sender.boxes.primary
            )))
        }
        _ => Ok(()),
    }
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
        headline: headline.to_string(),
    };
    match (a.kind, &a.re) {
        (k, _) if k.is_hold() => ctx.store.put_hold(&entry())?,
        (Kind::Release, Some(re)) => ctx.store.remove_hold(re)?,
        (k, _) if k.creates_obligation() => ctx.store.put_owed(&entry())?,
        (Kind::Done, Some(re)) => {
            for b in sender.boxes.iter() {
                ctx.store.remove_owed(b, re)?;
            }
        }
        _ => {}
    }
    if !a.kind.is_control() {
        let pending = Pending {
            id: id.clone(),
            kind: Some(a.kind),
            to: mail.to.clone(),
            sent: Some(now),
            headline: headline.to_string(),
        };
        ctx.store.put_pending(&sender.boxes.primary, &pending)?;
    }
    Ok(())
}

/// Type the envelope into the wake pane. `None` when it was typed (and
/// submitted, unless asked not to); otherwise the reason it was not.
fn wake(
    tmux: Option<&Tmux>,
    pane: Option<&Pane>,
    envelope: &str,
    d: Delivery,
    mail: &Mail,
) -> Option<String> {
    let (Some(t), Some(p)) = (tmux, pane) else {
        return Some(if d.wake {
            mail.no_wake
                .clone()
                .unwrap_or_else(|| "no pane to wake".into())
        } else {
            "--no-wake".into()
        });
    };
    match (TypedEnvelope { tmux: t }).wake(p, envelope, d.submit) {
        Ok(Woken::Typed) => None,
        Ok(Woken::NotConfirmed) => Some(format!(
            "typed into {0} but not seen there after 2 s, so not submitted; hail read {0} 10 to check, then hail keys {0} Enter",
            p.id
        )),
        Err(e) => Some(format!("typing into {} failed: {e}", p.id)),
    }
}
