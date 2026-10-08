# hail — design

Agent-to-agent messaging for coding agents that share a machine. One-way sends,
bodies on disk, a one-line envelope in the recipient's prompt, receipts that mean
"the agent read it", and `await` built on top of receipts.

## Why it exists

Measured in `~/code/agent-audit/CODEX.md` (Sep 2026):

- Codex compaction keeps every **user** message verbatim (newest first, to a cap)
  and discards every tool output. Claude Code summarises everything.
- A `tmux-bridge message` was typed into the pane, so it was a user message. In the
  murail-dev pane 294 retained relays rode on every API call: ~60k of a 166k-token
  call, 36 %, for the life of the pane.
- Coordinators read the target pane ~4 times per message sent to learn whether the
  text landed and whether a reply came. Each read is a full-context API call.
- Bodies over ~400 chars could not be verified as typed, so rulings went out in two
  parts, and a truncated first part read as a complete different thought.

So: the body must not be a user message, delivery must be a fact the sender can
query without reading a pane, and the thing that *is* in the prompt must be small
and triageable.

## Principles (and where they come from)

1. **Send is one-way. Reply is a derived pattern.** Hewitt, Bishop & Steiger 1973:
   sending presupposes no answer; a reply is arranged by passing a continuation.
   `hail send` returns as soon as the envelope is delivered. The envelope's
   `reply:` field is the continuation. `hail await` is a separate verb that blocks
   on receipts; nothing in `send` waits.
2. **The receipt is end-to-end.** Saltzer, Reed & Clark 1984: only a check at the
   application endpoints proves the transfer; lower-level acks are performance.
   Keystroke verification proves text landed in a pane — that is the network ack.
   The receipt hail reports is written by the recipient's own `inbox` command,
   i.e. the agent has the body in its context. `sent`/`await` answer *that*.
3. **The prompt sees a triage line, not a summary.** SWE-agent 2024: every token in
   an LM's window competes for attention; feedback should be sufficient and no
   more. The envelope is ≤160 chars, kind first, one-line ask. The body is fetched
   on demand as tool output and vanishes at the next compaction.
4. **Durable body, ephemeral prompt.** The body outlives the pane, the compaction,
   and the tmux server. When a bead id is named the body is also posted to the
   bead, because rulings must be bead-durable before they are bridge-durable
   (herald coordinator, review 2026-09-04).
5. **Stop-class messages are complete in the envelope.** `stop`, `hold`, `nogo`,
   `announce` must never sit behind a fetch: a red gate or a hold-before-land has
   to act even if the agent never runs `inbox` (both coordinators, same review).
6. **Addresses are seats.** A seat is the workspace an agent works in: the jj
   workspace or git root, by name, or a `.hail-seat` file. It is derived from
   the working directory only. Pane ids shift after restarts, labels were set
   by hand and drifted, and every rule that read the process tree or an
   inherited `TMUX_PANE` named the wrong pane once the Codex app-server moved
   (murail-4vc8v, murail-m65jq, 0.3.7). The pane is a transport detail,
   resolved at send time from the panes sitting in the seat's directory.
   Several agents in one directory are told apart as `<seat>@<pane>`, a
   sub-seat accepted only for a Claude pane whose own directory is the seat.
   A sub-agent, which has no pane and no hooks, is `<seat>/<name>`: its mail
   goes to the parent's mailbox marked `for:`, and the parent relays it.
7. **Transport is a backend.** Typing into a tmux pane is how the envelope is
   delivered today. The protocol (envelope, inbox, receipt) does not know that.
   A Claude in-process message or a Codex app-server turn can deliver the same
   envelope later without changing the skill.
8. **No daemon.** Every verb is a short process (one Rust binary since 0.4)
   over files and tmux. State is files under `$XDG_STATE_HOME/hail`, outside
   every repo tree, because herald's fan-in gate hashes checkout contents.
   Each seat's inbox is a Maildir (`tmp/`, `new/`, `cur/`): the claim is a
   rename, so exactly one reader wins and the receipt (the claimed file, its
   name saying how and its mtime saying when) cannot exist before the claim.
   Hooks and receipt checks touch only the filesystem.

## Shape

```
  sender (seat murail-1a, pane %5)                   recipient (seat murail-1b, pane %7)
  ┌──────────────────────────────┐                   ┌──────────────────────────────┐
  │ agent decides to send        │                   │ agent's prompt gets ONE line │
  │                              │                   │                              │
  │ $ hail murail-1b ruling \    │                   │ [hail kind:ruling            │
  │     --bead murail-ke7is \    │                   │  from:murail-1a/%5           │
  │     <<'EOF'                  │    (2) envelope   │  reply:murail-1a             │
  │ convert at the receipt       │ ───────────────►  │  id:0905T171200-a3f1         │
  │ <body…>                      │  typed keystrokes │  bead:murail-ke7is] convert  │
  │ EOF                          │  verified once    │  at the receipt — hail inbox │
  │   id=0905T171200-a3f1        │                   │                              │
  └──────┬───────────┬───────────┘                   │ UserPromptSubmit hook (4)    │
         │(1)        │(3)                            │  hail deliver → body as      │
         │           │                               │  context, claimed            │
         ▼           ▼                               └──────────────┬───────────────┘
  ~/.local/state/hail/seats/murail-1b/            bd comment         │ (5) rename new → cur
    new/0905T171200-a3f1.md  ◄── body ───────  murail-ke7is #7      │
    cur/0905T171200-a3f1.injected.md ◄──────────────────────────────┘
         ▲
         │ (6)  $ hail sent 0905T171200-a3f1   →  injected 2026-09-05T00:14:09Z
         │      $ hail await 0905T171200-a3f1 --timeout 600
  sender, any time later, no pane read
```

1. Body written durably to the seat's Maildir (`tmp/` then renamed into `new/`).
2. Envelope typed into the seat's agent pane (copy mode left, dialog refused),
   verified, submitted. No agent pane: exit 5, the message waits in `new/`.
3. If a bead is named, the body is posted as a bead comment after the wake;
   failure is one warning.
4. The recipient's prompt hook (`hail deliver`) claims the body and puts it in
   context; without hooks, `hail inbox` does the same as tool output.
5. The claim is the receipt: `cur/<id>.<how>.md`, mtime = when. A body that
   is only the headline is injected as its envelope line.
6. Sender checks or waits on the receipt through the id index. Never reads the pane.

What survives compaction on the recipient side: the envelope (≤400 chars).
What does not: the body. What survives everything: the file and the bead comment.

## Code map

The numbers are the steps in Shape above. Files under `store/`, `envelope`,
`transport/dialog` and `hooks/setup` do no process or tmux I/O.

```
main.rs            argv -> early help/version -> clap -> migration gate -> commands::run
cli.rs, help.rs    the argument grammar (0.3 forms normalised) and the help pages
ctx.rs             where this process runs: store, cwd, home, its seat and mailboxes
seat.rs            identity: seat_of(dir) from .hail-seat, jj or git root; Addr
route.rs           what a target names, and where mail for it goes and who to wake
envelope.rs        kinds, the [hail ...] line, folding, bead detection         (2)
commands/send.rs   the send pipeline: compose, route, check, post, wake, bd    (1)-(3)
commands/receive.rs deliver (the hook), inbox, show; sent and await           (4)-(6)
commands/brief.rs  the standing state: unread, late sends, holds, obligations
commands/panes.rs  whoami, seats, list; read/type/keys for non-agent panes
commands/setup.rs, doctor.rs   install the hooks; check the install
hooks/             what the hooks run and print; the config merge (setup.rs)
store/             the Maildir (mailbox.rs), ids and their index (ids.rs), the
                   file format (message.rs), owed/holds/pending (records.rs),
                   the archive (gc.rs)                                          (1),(5)
transport/         tmux (tmux.rs), which panes run agents (agent.rs), panes to
                   seats (pane_map.rs), the dialog guard (dialog.rs), typing
                   and verifying the envelope (mod.rs)                          (2)
policy.rs          every limit and timeout, in one place
input.rs, out.rs   stdin that never hangs; stdout that ends quietly; tables
migrate.rs         the one-time import of 0.3 state, and its revert
bd.rs              the optional bead comment (a subprocess, after the wake)     (3)
```

## Verbs

```
hail <seat> <kind> [--re id] [--bead id] [--scope s] <<'EOF' … EOF   → id=…, bead=… comment=…
hail note <bead> '<headline>'                                         progress, on the bead only
hail sent <id> · await <id>... [--timeout s] [--any]                  receipts
hail brief · inbox [--peek] [--all] · show <id> · deliver --format h  receiving
hail whoami · seats · list                                            seats and panes
hail read <pane|seat> [n] · type … · keys …                           non-agent panes only
hail setup · doctor · migrate · gc · help · version                   the tool
```

`stop hold block release announce` type the full headline inline, with no body
and no fetch hint. The other kinds type the envelope and keep the body in the
inbox, except `fyi`: it is quiet wherever the recipient's hooks run, and
arrives with their next prompt. A typed message costs its reader a turn;
progress costs nobody anything on the bead (`hail note`).

## What is deliberately not here

- No polling helper for agents. `await` checks the id index every 100 ms (a few
  stats per id); it is the only verb that waits.
- No broadcast. A message has one recipient; a coordinator loops.
- No message history in the tool. `hail inbox --all` lists the files; the bead
  holds anything that matters.
- No JSON protocol yet. The envelope is a fixed bracketed header because it has
  to read as prose in a prompt.

## Compatibility

`tmux-bridge` stays as a symlink to `hail` for one week from install so live
panes finish their conversations; the old `message` verb is an alias of `send`.
The envelope prefix is `[hail ...]`; the skill tells agents to treat `[tb ...]`
and `[tmux-bridge ...]` the same way during the alias week.
