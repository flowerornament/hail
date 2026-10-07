//! hail — messages between coding agents that share a machine. See DESIGN.md
//! and docs/2026-10-06-rust-port-spec.md.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

/// Print to stdout; a reader that went away (`hail inbox | head -1`) ends the
/// output quietly instead of panicking.
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout(), $($t)*);
    }};
}

macro_rules! outln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

pub mod bd;
pub mod cli;
pub mod commands;
pub mod ctx;
pub mod envelope;
pub mod error;
pub mod help;
pub mod hooks;
pub mod migrate;
pub mod seat;
pub mod store;
pub mod time;
pub mod transport;

use clap::Parser;

use cli::{Cli, Cmd, Early};
use commands::send::SendArgs;
use ctx::Ctx;
use error::{Error, Result};

/// Run hail on these arguments; the return value is the exit code.
pub fn run(args: Vec<String>) -> u8 {
    match dispatch(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    }
}

fn dispatch(args: Vec<String>) -> Result<u8> {
    let args = match cli::early(args) {
        Early::Help(topic) => return help(&topic),
        Early::Version(json) => return version(json),
        Early::Parse(a) => a,
    };
    let cli = Cli::try_parse_from(&args).map_err(|e| {
        // clap's message without its usage block: the lines before "Usage:".
        let text = e.to_string();
        let what: Vec<&str> = text
            .lines()
            .map(str::trim)
            .take_while(|l| !l.starts_with("Usage:"))
            .filter(|l| !l.is_empty() && !l.starts_with("tip:"))
            .collect();
        let what = what.join(" ").trim_start_matches("error: ").to_string();
        Error::Usage(format!("{what}. 'hail help' lists every command"))
    })?;
    let ctx = Ctx::from_env();
    if ctx.store.legacy_present() && needs_migration(&cli.cmd) {
        return Err(Error::State(
            "hail 0.4 needs a one-time import of the 0.3 state first: run hail migrate".into(),
        ));
    }
    use commands::{brief, panes, receive, send, setup};
    match cli.cmd {
        Cmd::Send(s) => send::run(&ctx, send_args(s)?),
        Cmd::Sent { id } => receive::sent(&ctx, &id),
        Cmd::Await { ids, timeout, any } => receive::await_ids(&ctx, &ids, timeout, any),
        Cmd::Deliver { format } => receive::deliver(&ctx, format.as_deref()),
        Cmd::Brief { all, hook } => brief::run(&ctx, all, hook),
        Cmd::Inbox { peek, all } => receive::inbox(&ctx, peek, all),
        Cmd::Show { id } => receive::show(&ctx, &id),
        Cmd::Whoami => panes::whoami(&ctx),
        Cmd::Seats { seat } => panes::seats(&ctx, seat.as_deref()),
        Cmd::List => panes::list(&ctx),
        Cmd::Read { target, lines } => panes::read(&ctx, &target, lines.unwrap_or(50)),
        Cmd::Type { target, text } => panes::type_text(&ctx, &target, &text),
        Cmd::Keys { target, keys } => panes::keys(&ctx, &target, &keys),
        Cmd::Setup { check, yes } => setup::setup(check, yes),
        Cmd::Doctor => setup::doctor(&ctx),
        Cmd::Migrate { revert: false } => migrate::migrate(&ctx),
        Cmd::Migrate { revert: true } => migrate::revert(&ctx),
        Cmd::Gc { days } => migrate::gc(&ctx, days),
        Cmd::Help { topic } => help(topic.as_deref().unwrap_or("")),
        Cmd::Version { json } => version(json),
        Cmd::Name { args } => panes::name_shim(&ctx, args.first().map(String::as_str)),
        Cmd::Hello => panes::hello_shim(&ctx),
        Cmd::Who { seat } => panes::seats(
            &ctx,
            seat.as_deref()
                .or(ctx.seat_here()?.as_ref().map(|s| s.name.as_str())),
        ),
        Cmd::Resolve { seat } => panes::resolve_shim(&ctx, &seat),
        Cmd::Id => panes::id_shim(&ctx),
    }
}

/// Verbs that read or write mail refuse to run on unmigrated 0.3 state;
/// hooks stay silent instead (they check for themselves).
fn needs_migration(cmd: &Cmd) -> bool {
    !matches!(
        cmd,
        Cmd::Deliver { .. }
            | Cmd::Brief { .. }
            | Cmd::Migrate { .. }
            | Cmd::Help { .. }
            | Cmd::Version { .. }
            | Cmd::Doctor
            | Cmd::Setup { .. }
            | Cmd::Name { .. }
            | Cmd::List
            | Cmd::Whoami
            | Cmd::Id
    )
}

fn send_args(s: cli::SendCli) -> Result<SendArgs> {
    let legacy = s.kind.is_some();
    let (kind, headline) = if let Some(k) = s.kind {
        let headline = s.rest.first().cloned().ok_or_else(|| {
            Error::Usage("missing headline: hail <seat> <kind> '<headline>' (or the 0.3 form: hail <target> '<headline>' --kind <k>)".into())
        })?;
        if s.rest.len() > 1 {
            return Err(Error::Usage(format!(
                "unexpected argument '{}' after the headline",
                s.rest[1]
            )));
        }
        (k, Some(headline))
    } else {
        let Some(k) = s.rest.first().cloned() else {
            return Err(Error::Usage(format!(
                "--kind is missing: hail {} <kind> '<headline>' ({}). If you meant a command, 'hail help' lists them",
                s.target,
                envelope::kinds_list()
            )));
        };
        if !envelope::is_kind(&k) {
            return Err(Error::Usage(format!(
                "'{k}' is not a kind ({}). Usage: hail {} <kind> '<headline>'; if '{}' is a command, 'hail help' lists them",
                envelope::kinds_list(),
                s.target,
                s.target
            )));
        }
        (k, s.rest.get(1).cloned())
    };
    if s.body.is_some() && !legacy {
        return Err(Error::Usage(
            "--body belongs to the 0.3 form; put the body on stdin after the headline line".into(),
        ));
    }
    Ok(SendArgs {
        target: s.target,
        kind,
        headline,
        legacy_body: s.body,
        legacy,
        re: s.re,
        bead: s.bead,
        scope: s.scope,
        submit: !s.no_submit,
        force: s.force,
        wake: !s.no_wake,
    })
}

fn help(topic: &str) -> Result<u8> {
    match help::page(topic) {
        Some(p) => {
            out!("{p}");
            Ok(0)
        }
        None => Err(Error::Usage(format!(
            "no help for '{topic}'. Topics: send kinds receive seats panes setup state"
        ))),
    }
}

fn version(json: bool) -> Result<u8> {
    let v = env!("CARGO_PKG_VERSION");
    if json {
        outln!("{}", serde_json::json!({ "name": "hail", "version": v }));
    } else {
        outln!("hail {v}");
    }
    Ok(0)
}
