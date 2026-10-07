//! Arguments. `hail <seat> ...` is short for `hail send <seat> ...`; help and
//! version are handled before clap so every page is ours.

use clap::{Args, Parser, Subcommand};

use crate::commands::send::{Delivery, Form, SendArgs};
use crate::envelope::Kind;
use crate::error::{Error, Result};
use crate::store::ids::Id;

#[derive(Parser, Debug)]
#[command(
    name = "hail",
    disable_help_subcommand = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    #[command(alias = "message", alias = "msg")]
    Send(SendCli),
    Sent {
        id: String,
    },
    Await {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        #[arg(long)]
        any: bool,
    },
    Deliver {
        #[arg(long, value_parser = ["claude", "codex"])]
        format: Option<String>,
    },
    Brief {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        hook: bool,
    },
    Inbox {
        #[arg(long)]
        peek: bool,
        #[arg(long)]
        all: bool,
    },
    Show {
        id: String,
    },
    Whoami,
    Seats {
        seat: Option<String>,
    },
    List,
    Read {
        target: String,
        lines: Option<usize>,
    },
    Type {
        target: String,
        text: String,
    },
    Keys {
        target: String,
        #[arg(required = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    Setup {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        yes: bool,
    },
    Doctor,
    Migrate {
        #[arg(long)]
        revert: bool,
    },
    Gc {
        #[arg(long, default_value_t = crate::policy::GC_DAYS)]
        days: u64,
    },
    Help {
        topic: Option<String>,
    },
    Version {
        #[arg(long)]
        json: bool,
    },
    // 0.3 identity verbs, shims through 0.4.
    Name {
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Hello,
    Who {
        seat: Option<String>,
    },
    Resolve {
        seat: String,
    },
    Id,
}

#[derive(Args, Debug)]
pub struct SendCli {
    pub target: String,
    /// New form: `<kind> [headline]`. With --kind (0.3 form): `<headline>`.
    #[arg(num_args = 0..=2)]
    pub rest: Vec<String>,
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub body: Option<String>,
    #[arg(long)]
    pub re: Option<String>,
    #[arg(long)]
    pub bead: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub scope: Option<String>,
    #[arg(long)]
    pub no_submit: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub no_wake: bool,
}

pub const VERBS: &[&str] = &[
    "send", "message", "msg", "sent", "await", "deliver", "brief", "inbox", "show", "whoami",
    "seats", "list", "read", "type", "keys", "setup", "doctor", "migrate", "gc", "help", "version",
    "name", "hello", "who", "resolve", "id",
];

/// What to do before clap sees the arguments.
pub enum Early {
    Help(String),
    Version(bool),
    Parse(Vec<String>),
}

pub fn early(mut args: Vec<String>) -> Early {
    if args.len() <= 1 {
        return Early::Help(String::new());
    }
    let first = args[1].clone();
    if matches!(first.as_str(), "-h" | "--help") {
        return Early::Help(String::new());
    }
    if matches!(first.as_str(), "-V" | "--version") {
        return Early::Version(false);
    }
    if args[1..].iter().any(|a| a == "-h" || a == "--help") {
        return Early::Help(first);
    }
    if !first.starts_with('-') && !VERBS.contains(&first.as_str()) {
        args.insert(1, "send".into());
    }
    Early::Parse(args)
}

/// clap's message without its usage block: the lines before "Usage:".
pub fn clap_message(e: &clap::Error) -> String {
    let text = e.to_string();
    let what: Vec<&str> = text
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with("Usage:"))
        .filter(|l| !l.is_empty() && !l.starts_with("tip:"))
        .collect();
    let what = what.join(" ");
    format!(
        "{}. 'hail help' lists every command",
        what.trim_start_matches("error: ")
    )
}

impl Cmd {
    /// Verbs that read or write mail refuse to run on unmigrated 0.3 state;
    /// hooks stay silent instead (they check for themselves).
    pub const fn needs_migrated_state(&self) -> bool {
        !matches!(
            self,
            Self::Deliver { .. }
                | Self::Brief { .. }
                | Self::Migrate { .. }
                | Self::Help { .. }
                | Self::Version { .. }
                | Self::Doctor
                | Self::Setup { .. }
                | Self::Name { .. }
                | Self::List
                | Self::Whoami
                | Self::Id
        )
    }
}

impl SendCli {
    /// The new form is `<seat> <kind> [headline]`; with `--kind`, the 0.3
    /// form `<target> <headline>`.
    pub fn into_args(self) -> Result<SendArgs> {
        let mut rest = self.rest.into_iter();
        let (kind, form) = if let Some(k) = self.kind {
            let headline = rest.next().ok_or_else(|| {
                Error::Usage(
                    "missing headline: hail <seat> <kind> '<headline>' (or the 0.3 form: hail <target> '<headline>' --kind <k>)".into(),
                )
            })?;
            (
                k,
                Form::Legacy {
                    headline,
                    body: self.body,
                },
            )
        } else {
            if self.body.is_some() {
                return Err(Error::Usage(
                    "--body belongs to the 0.3 form; put the body on stdin after the headline line"
                        .into(),
                ));
            }
            let k = rest.next().ok_or_else(|| {
                Error::Usage(format!(
                    "--kind is missing: hail {} <kind> '<headline>' ({}). If you meant a command, 'hail help' lists them",
                    self.target,
                    Kind::list()
                ))
            })?;
            (
                k,
                Form::Current {
                    headline: rest.next(),
                },
            )
        };
        if let Some(extra) = rest.next() {
            return Err(Error::Usage(format!(
                "unexpected argument '{extra}' after the headline"
            )));
        }
        let kind = Kind::parse(&kind).ok_or_else(|| {
            Error::Usage(format!(
                "'{kind}' is not a kind ({}). Usage: hail {} <kind> '<headline>'; if '{}' is a command, 'hail help' lists them",
                Kind::list(),
                self.target,
                self.target
            ))
        })?;
        let re = self
            .re
            .map(|r| Id::parse(&r).ok_or_else(|| Error::Usage("--re must be a message id".into())))
            .transpose()?;
        Ok(SendArgs {
            target: self.target,
            kind,
            form,
            re,
            bead: self.bead,
            scope: self.scope,
            delivery: Delivery {
                wake: !self.no_wake,
                submit: !self.no_submit,
                force: self.force,
            },
        })
    }
}
