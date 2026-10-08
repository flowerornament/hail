//! hail — messages between coding agents that share a machine.
//!
//! Start with DESIGN.md: "Shape" follows one message from send to receipt in
//! six steps, and "Code map" names the module behind each step.
//!
//! Words used throughout:
//! - **seat**: an agent's address, the name of the workspace it works in
//!   (`seat.rs`). A **sub-seat**, `<seat>@<pane>`, is one Claude pane in a
//!   directory several agents share. Either one is an `Addr`.
//! - **mailbox**: the Maildir that holds a seat's or sub-seat's mail, under
//!   `$XDG_STATE_HOME/hail/seats/<addr>/` (`store/`).
//! - **message**: a headline (one line, capped) and a body (any length).
//!   The **envelope** is the one `[hail …]` line typed into the recipient's
//!   pane. Typing it is the **wake**. The body is fetched separately.
//! - **claim**: a hook or `hail inbox` renames the message from `new/` to
//!   `cur/`. That rename is the **receipt** `hail sent` and `hail await` read.
//! - **kind**: what a message asks (`envelope::Kind`). **Control kinds**
//!   (stop, hold, block, release, announce) are complete in the envelope.
//!   An **obligation** (left by ruling, go, ask) stays on the recipient until
//!   it sends `done --re <id>`. A **hold** or block stays in effect until a
//!   `release`.
//! - **harness**: the agent program, Claude Code or Codex. Its **hooks** run
//!   `hail deliver` and `hail brief` (`hooks/`).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

#[macro_use]
mod out;

mod cli;
mod commands;
mod ctx;
mod envelope;
mod error;
mod help;
mod hooks;
mod input;
mod migrate;
mod policy;
mod route;
mod seat;
mod store;
mod time;
mod transport;

use clap::Parser;

use cli::{Cli, Early};
use ctx::Ctx;
use error::{Error, Result};

fn main() -> std::process::ExitCode {
    let code = match run(std::env::args().collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    };
    std::process::ExitCode::from(code)
}

fn run(args: Vec<String>) -> Result<u8> {
    let args = match cli::early(args) {
        Early::Help(topic) => return commands::help(&topic),
        Early::Version(json) => return Ok(commands::version(json)),
        Early::Parse(a) => a,
    };
    let cli = Cli::try_parse_from(&args).map_err(|e| Error::Usage(cli::clap_message(&e)))?;
    let ctx = Ctx::from_env();
    if ctx.store.legacy_present() && cli.cmd.needs_migrated_state() {
        return Err(Error::State(
            "hail 0.4 needs a one-time import of the 0.3 state first: run hail migrate".into(),
        ));
    }
    commands::run(&ctx, cli.cmd)
}
