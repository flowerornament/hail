//! Arguments. `hail <seat> ...` is short for `hail send <seat> ...`; help and
//! version are handled before clap so every page is ours.

use clap::{Args, Parser, Subcommand};

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
        #[arg(long, default_value_t = 90)]
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
