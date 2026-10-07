//! hail — messages between coding agents that share a machine. See DESIGN.md
//! and docs/2026-10-06-rust-port-spec.md.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

#[macro_use]
mod out;

mod bd;
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
