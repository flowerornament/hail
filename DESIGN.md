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
6. **Addresses are labels.** Pane ids shift after restarts and misrouted a ruling
   once. Inbox directories and `from:` use the pane's label; the pane id is a
   transport detail resolved at send time and carried in `reply:` for convenience.
7. **Transport is a backend.** Typing into a tmux pane is how the envelope is
   delivered today. The protocol (envelope, inbox, receipt) does not know that.
   A Claude in-process message or a Codex app-server turn can deliver the same
   envelope later without changing the skill.
8. **No daemon.** Every verb is a short bash process over files and tmux. State
   is files under `$XDG_STATE_HOME/hail`, outside every repo tree, because
   herald's fan-in gate hashes checkout contents.

## Shape

```
  sender pane (label: murail-1a, %5)                 recipient pane (label: murail-1b, %7)
  ┌──────────────────────────────┐                   ┌──────────────────────────────┐
  │ agent decides to send        │                   │ agent's prompt gets ONE line │
  │                              │                   │                              │
  │ $ hail send murail-1b \      │                   │ [hail kind:ruling            │
  │     'convert at the receipt' │                   │   from:murail-1a/%5 reply:%5 │
  │     --kind ruling \          │    (3) envelope   │   id:0905T1712-a3f1          │
  │     --bead murail-ke7is \    │ ───────────────►  │   bead:murail-ke7is]         │
  │     --body ruling.md         │  typed keystrokes │  convert at the receipt      │
  │   id=0905T1712-a3f1          │  verified once    │  — hail inbox                │
  │   bead=murail-ke7is comment=7│                   │                              │
  └──────┬───────────┬───────────┘                   │ $ hail inbox        (4)      │
         │(1)        │(2)                            │   from: murail-1a/%5 ...     │
         │           │                               │   <full body, tool output>   │
         ▼           ▼                               └──────────────┬───────────────┘
  ~/.local/state/hail/inbox/murail-1b/            bd comment         │ (5) writes .read
    0905T1712-a3f1.md   ◄───── body ──────────  murail-ke7is #7      │
    0905T1712-a3f1.read ◄────────────────────────────────────────────┘
         ▲
         │ (6)  $ hail sent 0905T1712-a3f1   →  read 2026-09-05T00:14:09Z
         │      $ hail await 0905T1712-a3f1 --timeout 600   (blocks on the .read file)
  sender, any time later, no pane read
```

1. Body written to the recipient's inbox directory (keyed on label).
2. If a bead is named, body posted as a bead comment; failure is one warning.
3. Envelope typed into the pane and verified (vim Normal-mode repair, one retry).
4. Recipient runs `hail inbox`; the body enters context as tool output.
5. `inbox` writes the receipt.
6. Sender checks or waits on the receipt. Never reads the pane.

What survives compaction on the recipient side: the envelope (≤160 chars).
What does not: the body. What survives everything: the file and the bead comment.

## Verbs

```
hail send <target> <ask> [--kind k] [--bead id] [--body file|-]   → id=..., bead=... comment=...
hail inbox [--peek] [--all]                                       recipient: fetch + receipt
hail sent <id>                                                    delivered | read <time> | unknown
hail await <id>... [--timeout s] [--any]                          block until read (all, or any)
hail list | name <pane> <label> | resolve <label> | id            addressing
hail read <target> [n] | type <target> <text> | keys <target> k   non-agent panes only
hail doctor | version
```

`stop hold nogo announce` type the full ask inline and skip the fetch hint.
`ruling go ask fyi` type the envelope and put the full text in the inbox.

## What is deliberately not here

- No polling helper. `await` blocks on the receipt file with inotify/fswatch when
  available and a 1 s sleep otherwise; it is the only verb that waits.
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
