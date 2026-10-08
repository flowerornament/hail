//! `hail --help` is the map; `hail help <topic>` and `hail <verb> --help` are
//! the pages. Written for agents: every example works when pasted.

pub const MAP: &str = "\
hail — messages between coding agents on one machine

Send (the body is everything after the first line):
  hail <seat> <kind> <<'EOF'      Write the message to the seat's inbox and
  <headline>                      type a one-line envelope into its agent's
  <body…>                         pane. Prints id=<id>.
  EOF
  hail <seat> <kind> '<headline>'
                                  Headline only; no body.
  sent <id>                       Receipt: delivered | injected <t> | read <t> | inline <t> | unknown
  await <id>... [--timeout S]     Block until every id (--any: any id) has a receipt

Receive:
  brief                           What you owe, what you were sent, holds in effect
  inbox [--peek] [--all]          Unread bodies (the hook usually delivers them)
  show <id>                       One message by id, from any seat

Seats (a seat is the workspace directory an agent works in):
  whoami                          This directory's seat
  seats [seat]                    Every seat: agent panes, unread, open obligations
  list                            Every tmux pane with its seat

Panes (for shells and processes, not agents):
  read <pane|seat> [N]  ·  type <pane|seat> <text>  ·  keys <pane|seat> <key>...

Tool:
  setup [--check] [--yes]         Install the Claude Code and Codex hooks
  doctor                          Check everything; each problem names its fix
  migrate [--revert]              One-time import of 0.3 state
  gc [--days N]                   Archive read mail older than N days (default 90)
  help [topic] · version          Topics: send kinds receive seats panes setup state

Examples:
  hail api-1b ask <<'EOF'
  Review src/auth.ts before the merge; reply done with your verdict
  The refresh path is auth/refresh.rs:40-120.
  EOF
  hail api-1a done --re 1006T171200-a3f1 'committed on abc123'
  hail await 1006T171200-a3f1 --timeout 900
";

pub const SEND: &str = "\
hail send — deliver a message to a seat

  hail <seat> <kind> [options] <<'EOF'
  <headline: the ask and the why, one line>
  <body: any detail, any length; optional>
  EOF
  hail <seat> <kind> '<headline>' [options]

<seat> is a workspace name (api-1b), a sub-seat in a shared directory
(hail@%28), a pane (%7, or api-1a/%5 as from: prints it), or a sub-agent
(web-1b/scout), whose mail goes to its parent marked for: and is relayed.
'hail seats' lists them. With a headline argument
stdin is never read. In a heredoc, quote the delimiter (<<'EOF') so nothing
in the body is expanded.

Options:
  --re <id>       The message this answers, closes (done) or lifts (release)
  --scope <s>     What it applies to: an issue, ref, path or action (one line)
  --no-submit     Type the envelope but do not press Enter
  --force         Skip the dialog guard. Only after 'hail read <seat> 10'.
  --as <name>     Sign as your sub-agent <seat>/<name>; replies come back for it.

The headline is capped at HAIL_ENVELOPE_MAX characters (400). A longer one is
folded at a sentence boundary and the full text rides in the body; a control
kind (stop hold block release announce) is refused instead (exit 2). Write
plain sentences.

Envelope typed into the recipient's pane:
  [hail kind:<k> from:<seat>/<pane> reply:<seat> id:<id> [for:] [re:] [scope:] [until:]] <headline>[ — hail inbox]
Reply to the reply: value. for:<name> marks mail for your sub-agent: relay it.

An fyi is quiet: it is not typed, and arrives with the recipient's next prompt
(typed as before where their hooks have never run). Progress belongs in your
issue tracker, not in someone's prompt.

Exit: 0 typed and submitted, or a quiet fyi; 5 written to the inbox but not
typed (no agent pane, or typing not confirmed): do NOT resend. It arrives on
their next prompt, unless hail says no hook reads that seat (then check hail
seats for where its agent works); 3 seat problem; 4 the target shows a
permission dialog.

The 0.3 form still works: hail <target> '<headline>' --kind <k> [--body text|file|-]
";

pub const KINDS: &str = "\
hail kinds — what each kind means and does

  ruling  go  ask     Leave an obligation on the recipient until it sends
                      'done --re <id>'. 'hail brief' lists them.
  done                Close one obligation: --re <id> required.
  hold  block         In effect until 'release --re <id>', or until it lapses:
                      a hold after 8h, a block after 7d, or --for 30m|8h|3d
                      (at most 7d). For a person's decision, not a lock:
                      landing and installs have their own locks.
  release             Lift a hold: --re <id> required.
  nogo  announce  stop
                      No state.
  fyi                 No state, and quiet: not typed; it arrives with the
                      recipient's next prompt. For outcomes read later;
                      progress belongs in the issue tracker.

stop hold block release announce are control kinds: typed in full, no body,
act on them at once. A message with no body is complete in its envelope.
";

pub const RECEIVE: &str = "\
hail receive — what was sent to you

With the hooks installed (hail setup), a body reaches you on the turn its
envelope lands, as hook context, and the receipt is written. Do not also run
hail inbox.

  brief [--all]         Unread envelopes, your sends with no receipt after 2
                        minutes, holds in effect, obligations on you. Five per
                        section; --all for everything.
  inbox [--peek] [--all]
                        Unread bodies, marked read. --peek leaves them unread.
  show <id>             One message by id from any seat; claims nothing.
  deliver --format claude|codex
                        For the UserPromptSubmit hook: unread bodies as hook
                        JSON, marked injected. Silent when there are none.
";

pub const SEATS: &str = "\
hail seats — who is who

A seat is the workspace an agent works in: the nearest .hail-seat file (its
content is the name), else the jj workspace root or git root, by basename.
It comes from the working directory, never from TMUX_PANE: run hail from
your workspace.

When several agents share one directory, each Claude pane is a sub-seat,
<seat>@<pane> (hail@%28); a Codex agent needs its own jj workspace.

A sub-agent (a Task agent, a Codex sub-agent) shares its parent's seat. It is
addressed <seat>/<name> through the parent, signs with --as <name>, and runs
no verb that reads mail (inbox, deliver, brief): those take the parent's mail.

  whoami          This directory's seat and where it came from
  seats [seat]    Every seat: agent panes, unread, open obligations, how long
                  ago a prompt hook read it (HOOK; - means nothing delivers
                  its mail), root
  list            Every tmux pane: target, session, size, process, seat, cwd
";

pub const PANES: &str = "\
hail panes — driving a shell or process (not an agent)

  read <pane|seat> [N]    Last N lines (default 50). Required before type/keys.
  type <pane|seat> <text> Type without Enter, verified.
  keys <pane|seat> <key>… Enter, Escape, C-c, Up, …

To reach an agent, send a message: hail help send.
";

pub const SETUP: &str = "\
hail setup — hooks and health

  setup           Show the change to ~/.claude/settings.json and
                  ~/.codex/config.toml, then apply it (asks; --yes to skip).
                  Replaces older hail lines; leaves everything else alone.
  setup --check   Exit 1 when either file needs a change.
  doctor          One line per check, each problem with its fix.

Codex asks you to trust changed hooks once: run /hooks in Codex. Both
harnesses read hooks at session start.
";

pub const STATE: &str = "\
hail state — files, environment, exit codes

State ($XDG_STATE_HOME/hail, default ~/.local/state/hail):
  seats/<seat>/new/<id>.md          unread
  seats/<seat>/cur/<id>.<how>.md    claimed (how: injected, read, inline; mtime = when)
  seats/<seat>/owed/<id>            obligations on the seat
  seats/<seat>/pending/<id>         sends with no receipt yet
  holds/<id>                        holds and blocks (expires: says when each lapses)
  holds/lapsed/<id>                 lapsed ones, after their issuer's brief said so
  ids/<id> -> <seat>                the id index
  archive/                          gc'd mail and the 0.3 tree

Environment:
  HAIL_ENVELOPE_MAX     headline cap (default 400)
  HAIL_SOCKET           tmux socket (TMUX_BRIDGE_SOCKET accepted)
  HAIL_SEAT             a seat name, only where the directory names none
  HAIL_AGENT_COMMANDS   commands that count as agents (default claude,codex)
  XDG_STATE_HOME        state root

Exit codes: 0 ok · 1 usage or state · 2 control headline over cap ·
3 seat problem · 4 dialog in the target · 5 in the inbox, not typed
";

/// The page for a topic or verb, or None.
pub fn page(topic: &str) -> Option<&'static str> {
    Some(match topic {
        "send" | "message" | "msg" | "sent" | "await" => SEND,
        "kinds" | "kind" => KINDS,
        "receive" | "deliver" | "brief" | "inbox" | "show" => RECEIVE,
        "seats" | "seat" | "whoami" | "list" | "identity" | "name" | "hello" | "who"
        | "resolve" | "id" => SEATS,
        "panes" | "pane" | "read" | "type" | "keys" => PANES,
        "setup" | "doctor" | "hooks" => SETUP,
        "state" | "env" | "environment" | "exit" | "migrate" | "gc" => STATE,
        "" | "help" | "version" => MAP,
        _ => return None,
    })
}
