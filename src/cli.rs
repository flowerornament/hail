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
    /// Removed in 0.5.1; says what to use instead. Drop it in 0.6.
    #[command(hide = true)]
    Note {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
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
    // Old identity verbs: shims that never fail (see commands/panes.rs).
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
    /// Accepted and ignored: hail no longer talks to bd. Kept so sends from
    /// sessions taught the flag still go through; drop it in 0.6.
    #[arg(long, hide = true)]
    pub bead: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub scope: Option<String>,
    #[arg(long)]
    pub no_submit: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub no_wake: bool,
    /// Sign as a sub-agent of this seat: `<seat>/<name>`.
    #[arg(long = "as", value_name = "NAME")]
    pub as_name: Option<String>,
    /// How long a hold or block lasts: `30m`, `8h`, `3d`.
    #[arg(long = "for", value_name = "SPAN")]
    pub hold_for: Option<String>,
}

/// `--for`: a span no longer than [`HOLD_MAX`](crate::policy::HOLD_MAX).
fn parse_hold_for(s: &str) -> Result<std::time::Duration> {
    let max = crate::policy::HOLD_MAX;
    match crate::time::parse_span(s) {
        Some(d) if d <= max => Ok(d),
        Some(_) => Err(Error::Usage(format!(
            "--for {s}: at most {}d; a hold is a person's decision, not a standing lock",
            max.as_secs() / 86_400
        ))),
        None => Err(Error::Usage(format!(
            "--for {s}: a span such as 30m, 8h or 3d"
        ))),
    }
}

pub const VERBS: &[&str] = &[
    "send", "message", "msg", "note", "sent", "await", "deliver", "brief", "inbox", "show",
    "whoami", "seats", "list", "read", "type", "keys", "setup", "doctor", "migrate", "gc", "help",
    "version", "name", "hello", "who", "resolve", "id",
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
        if let Some(n) = self
            .as_name
            .as_deref()
            .filter(|n| !crate::seat::valid_name(n))
        {
            return Err(Error::Usage(format!(
                "--as {n}: a sub-agent name is letters, digits, '.', '_' and '-'"
            )));
        }
        let hold_for = self.hold_for.as_deref().map(parse_hold_for).transpose()?;
        if hold_for.is_some() && !kind.is_hold() {
            return Err(Error::Usage(format!(
                "--for applies to hold and block, not {kind}"
            )));
        }
        if let Some(b) = &self.bead {
            eprintln!(
                "hail: --bead is ignored now (hail no longer posts to bd); name {b} in the headline if it matters"
            );
        }
        Ok(SendArgs {
            target: self.target,
            kind,
            form,
            re,
            scope: self.scope,
            as_name: self.as_name,
            hold_for,
            delivery: Delivery {
                wake: !self.no_wake,
                submit: !self.no_submit,
                force: self.force,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help;

    /// Values for the placeholders the docs use. An example with any other
    /// `<…>` outside quotes fails, so a new placeholder is added here on
    /// purpose rather than slipping past the check.
    const PLACEHOLDERS: &[(&str, &str)] = &[
        ("<seat>", "api-1b"),
        ("<issuer>", "api-1a"),
        ("<kind>", "ask"),
        ("<id>...", "1006T171200-a3f1"),
        ("<id>", "1006T171200-a3f1"),
        ("[options]", ""),
    ];

    /// The example commands agents copy: lines of the help pages and of the
    /// skill's code blocks that start with `hail `, and the skill's inline
    /// `` `hail …` `` spans of three words or more (two words is a mention of
    /// a verb, such as `hail inbox`, not an example).
    fn examples() -> Vec<String> {
        let mut out = Vec::new();
        let pages = [
            help::MAP,
            help::SEND,
            help::KINDS,
            help::RECEIVE,
            help::SEATS,
            help::PANES,
            help::SETUP,
            help::STATE,
        ];
        for page in pages {
            out.extend(
                page.lines()
                    .filter(|l| l.starts_with("  ") && l.trim_start().starts_with("hail "))
                    .map(|l| command_part(l.trim_start())),
            );
        }
        let skill = include_str!("../skills/hail/SKILL.md");
        let mut in_code = false;
        for line in skill.lines() {
            if line.starts_with("```") {
                in_code = !in_code;
            } else if in_code && line.starts_with("hail ") {
                out.push(command_part(line));
            } else if !in_code {
                for span in line.split('`').skip(1).step_by(2) {
                    if span.starts_with("hail ") && span.split_whitespace().count() >= 3 {
                        out.push(span.to_string());
                    }
                }
            }
        }
        out
    }

    /// The command without its trailing comment, description column or
    /// heredoc: everything before `  ` (two spaces) or ` <<`.
    fn command_part(line: &str) -> String {
        let end = [line.find("  "), line.find(" <<")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(line.len());
        line[..end].trim_end().to_string()
    }

    /// Split like a shell does for these examples: whitespace, with single
    /// and double quotes grouping words. Each word says whether it was
    /// quoted: a quoted `'<headline>'` is text, not a placeholder.
    fn words(line: &str) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        let (mut cur, mut quote, mut quoted, mut any) = (String::new(), None, false, false);
        for c in line.chars() {
            match (quote, c) {
                (None, '\'' | '"') => (quote, quoted, any) = (Some(c), true, true),
                (Some(q), c) if c == q => quote = None,
                (None, c) if c.is_whitespace() => {
                    if any {
                        out.push((std::mem::take(&mut cur), quoted));
                        (quoted, any) = (false, false);
                    }
                }
                (_, c) => {
                    cur.push(c);
                    any = true;
                }
            }
        }
        if any {
            out.push((cur, quoted));
        }
        out
    }

    #[test]
    fn every_documented_example_parses() {
        let examples = examples();
        assert!(examples.len() >= 10, "found only {examples:?}");
        for ex in examples {
            let mut args: Vec<String> = Vec::new();
            for (w, quoted) in words(&ex) {
                let w = PLACEHOLDERS
                    .iter()
                    .find(|(p, _)| !quoted && w == *p)
                    .map_or(w, |(_, v)| (*v).to_string());
                assert!(
                    quoted || !(w.starts_with('<') || w.starts_with('[')),
                    "{ex}: unknown placeholder {w}; add it to PLACEHOLDERS"
                );
                if !w.is_empty() {
                    args.push(w);
                }
            }
            let Early::Parse(args) = early(args) else {
                continue;
            };
            let cli =
                Cli::try_parse_from(&args).unwrap_or_else(|e| panic!("{ex}: {}", clap_message(&e)));
            if let Cmd::Send(s) = cli.cmd
                && let Err(e) = s.into_args()
            {
                panic!("{ex}: {e}");
            }
        }
    }
}
