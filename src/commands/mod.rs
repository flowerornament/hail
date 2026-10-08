//! One module per group of verbs, and the dispatch from a parsed command.

pub mod brief;
pub mod doctor;
pub mod note;
pub mod panes;
pub mod receive;
pub mod send;
pub mod setup;

use crate::cli::Cmd;
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::{help, migrate, store};

pub fn run(ctx: &Ctx, cmd: Cmd) -> Result<u8> {
    match cmd {
        Cmd::Send(s) => send::run(ctx, &s.into_args()?),
        Cmd::Note {
            bead,
            headline,
            as_name,
        } => note::run(ctx, &bead, headline, as_name.as_deref()),
        Cmd::Sent { id } => Ok(receive::sent(ctx, &id)),
        Cmd::Await { ids, timeout, any } => Ok(receive::await_ids(ctx, &ids, timeout, any)),
        Cmd::Deliver { format } => Ok(receive::deliver(ctx, format.as_deref())),
        Cmd::Brief { all, hook } => brief::run(ctx, all, hook),
        Cmd::Inbox { peek, all } => receive::inbox(ctx, peek, all),
        Cmd::Show { id } => receive::show(ctx, &id),
        Cmd::Whoami => panes::whoami(ctx),
        Cmd::Seats { seat } => panes::seats(ctx, seat.as_deref()),
        Cmd::List => panes::list(ctx),
        Cmd::Read { target, lines } => panes::read(ctx, &target, lines.unwrap_or(50)),
        Cmd::Type { target, text } => panes::type_text(ctx, &target, &text),
        Cmd::Keys { target, keys } => panes::keys(ctx, &target, &keys),
        Cmd::Setup { check, yes } => setup::setup(check, yes),
        Cmd::Doctor => Ok(doctor::run(ctx)),
        Cmd::Migrate { revert: false } => migrate::migrate(ctx),
        Cmd::Migrate { revert: true } => migrate::revert(ctx),
        Cmd::Gc { days } => gc(ctx, days),
        Cmd::Help { topic } => help(topic.as_deref().unwrap_or("")),
        Cmd::Version { json } => Ok(version(json)),
        // Old identity verbs: shims that never fail (see panes.rs).
        Cmd::Name { args } => Ok(panes::name_shim(ctx, args.first().map(String::as_str))),
        Cmd::Hello => panes::hello_shim(ctx),
        Cmd::Who { seat } => {
            let here = ctx.seat_here()?.map(|s| s.name);
            panes::seats(ctx, seat.as_deref().or(here.as_deref()))
        }
        Cmd::Resolve { seat } => panes::resolve_shim(ctx, &seat),
        Cmd::Id => panes::id_shim(ctx),
    }
}

fn gc(ctx: &Ctx, days: u64) -> Result<u8> {
    let moved = store::gc::archive(&ctx.store, days)?;
    outln!(
        "archived {moved} read messages older than {days} days into {}",
        ctx.store.archive_dir().display()
    );
    let now = crate::time::now();
    let cutoff = crate::time::before(
        now,
        std::time::Duration::from_secs(days.saturating_mul(86_400)),
    );
    let swept = ctx.store.sweep_holds(now, cutoff);
    outln!("removed {swept} lapsed holds (older than {days} days, or from a seat that is gone)");
    Ok(0)
}

pub fn help(topic: &str) -> Result<u8> {
    let page = help::page(topic).ok_or_else(|| {
        Error::Usage(format!(
            "no help for '{topic}'. Topics: send kinds receive seats panes setup state"
        ))
    })?;
    out!("{page}");
    Ok(0)
}

pub fn version(json: bool) -> u8 {
    let v = env!("CARGO_PKG_VERSION");
    if json {
        outln!("{}", serde_json::json!({ "name": "hail", "version": v }));
    } else {
        outln!("hail {v}");
    }
    0
}
