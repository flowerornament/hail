---
name: hail
description: "Agent-to-agent messaging over tmux with hail. Use this skill whenever the user mentions sending a message to another agent or pane, a `[hail ...]` (or `[tb ...]`/`[tmux-bridge ...]`) envelope appears in your prompt, you need to know whether another agent has read something, or you must drive a non-agent tmux pane (a shell, a running process). Includes the hail CLI: send, inbox, sent, await, labels, and a minimal raw-tmux fallback."
metadata:
  { "openclaw": { "emoji": "📯", "os": ["darwin", "linux"], "requires": { "bins": ["tmux", "hail"] } } }
---

# hail

One-way messages between coding agents that share a machine. The body goes to a
file; a one-line envelope goes into the other agent's prompt; the recipient's own
`hail inbox` writes the receipt; `hail sent` / `hail await` read it. Nothing waits
inside `send`, and nobody reads a pane to find out whether a message landed.

## If you see an envelope, run `hail inbox`

An envelope looks like this, as one line in your prompt:

```
[hail kind:ruling from:murail-1a/%5 reply:%5 id:0905T1712-a3f1 bead:murail-ke7is] convert at the receipt… — hail inbox
```

- `kind` — what to do with it (see Kinds). `from` — sender's label/pane.
  `reply` — the pane to answer. `id` — for receipts. `bead` — the issue the body
  is also posted to, when there is one. Then a one-line ask, truncated with `…`.
- **Run `hail inbox`.** It prints every unread body as tool output and writes the
  receipt the sender is waiting on. Never act on a `ruling`, `go`, `ask` or `fyi`
  from the envelope alone: the envelope is a pointer with a why, not the ruling.
- The body is tool output, so it vanishes at your next compaction; the file and
  the bead comment do not. `hail inbox --all` re-reads everything.
- **Alias week:** for one week after install, `[tb ...]` and `[tmux-bridge ...]`
  envelopes mean exactly the same thing as `[hail ...]`. Treat them identically;
  `hail inbox` fetches them too.

## Kinds

`ruling go nogo announce ask fyi stop hold` (default `ask`).

- **Inline, complete in the envelope:** `stop`, `hold`, `nogo`, `announce`. The
  full text is typed into the pane with no fetch hint. Act on it immediately, even
  if you never run `inbox`; a red gate or a hold-before-land cannot sit behind a
  fetch. (The body is still written to your inbox for the record.)
- **Envelope + fetch:** `ruling`, `go`, `ask`, `fyi`. The pane gets ≤160 chars;
  the full text is in your inbox.

## Sending: send, await, sent

```bash
hail send <target> <ask> [--kind k] [--bead id] [--body file|-]
#   → id=0905T1712-a3f1            (and bead=<id> comment=<n> when posted)
hail sent <id>                     # delivered | read <time> | unknown
hail await <id>... [--timeout SECONDS] [--any]
#   blocks until every id (or any, with --any) has a receipt; default 600 s
#   prints one line per id: "<id> read <time>" | "<id> timeout" | "<id> pending"
#   exit 0 when satisfied, 1 on timeout. Needs no tmux server.
```

`<target>` is a label (preferred) or a pane id. `<ask>` is the one-line summary
that goes in the envelope; `--body` supplies a longer body from a file or stdin
(`-`); without it the ask is the body. `send` verifies the envelope landed
(vim-Normal-mode repair, one retry) and returns. It presses nothing: submit with
`hail keys <target> Enter` if the recipient's composer needs it.

Typical exchange:

```bash
hail read codex 5                                        # read guard (see below)
id=$(hail send codex 'Review src/auth.ts against murail-ke7is; verdict on the bead' --kind ask)
hail keys codex Enter
id=${id#id=}
# ... do other work ...
hail await "$id" --timeout 900                           # blocks until codex ran `hail inbox`
hail sent "$id"                                          # or just check, any time later
```

Rules for senders:

- **Do not read the target pane** to see whether the text landed (`send` verified
  it) or to look for a reply (replies arrive in *your* pane as envelopes, and
  `await`/`sent` tell you when yours was read).
- **Do not poll.** `await` is the one verb that waits; use it, or move on.
- **One recipient per message.** A coordinator loops over targets.
- **Bead-first for rulings.** `--bead <id>` (or an issue id in the ask) posts the
  body as a bead comment and cites `bead:<id>` in the envelope. If `bd` is missing
  or fails you get one warning and the message goes file-only.
- Keep the ask short; it is what survives the recipient's compaction.

Replying is just another send, to the `reply:` pane (or better, the `from` label):

```bash
hail inbox                                               # fetch, write the receipt
hail read murail-1a 5
hail send murail-1a '87% line coverage; OAuth refresh path (142-168) uncovered' --kind fyi
hail keys murail-1a Enter
```

## Labels

Inbox directories and `from:` are keyed on the pane's label, because pane ids
shift after a restart. Label yourself first, then discover others:

```bash
hail name "$(hail id)" murail-1b      # label this pane (the inbox is keyed on it)
hail list                             # TARGET SESSION:WIN SIZE PROCESS LABEL CWD
hail resolve murail-1a                # → %5
```

An unlabeled pane's inbox is keyed on its id; that works but does not survive a
restart. Labels with `/` are stored with `_`.

## Non-agent panes: read, type, keys

For a plain shell or a running process, there is nobody to run `inbox`, so you
drive the pane directly and you *do* read it to see output. The CLI enforces a
read guard: `read` marks the pane, `type`/`keys`/`send` require the mark and
clear it, so every action is read → act → read.

```bash
hail read worker 10                   # see the prompt
hail type worker "y"                  # type (verified, no Enter)
hail read worker 10                   # verify
hail keys worker Enter                # submit; Escape, C-c, C-d, Up ... also work
hail read worker 20                   # see the result
```

```
$ hail type worker "y"
error: must read the pane before interacting. Run: hail read worker
```

`type` never presses Enter and cannot verify text past ~400 chars (a composer
shows only its last lines): put long content on a bead or in `--body` instead.

## State and environment

- Files: `$XDG_STATE_HOME/hail/inbox/<label>/<id>.md` and `<id>.read` (default
  `~/.local/state/hail`). Keep `XDG_STATE_HOME` outside every git checkout; the
  inbox is scratch state and must not be hashed by a repo gate.
- `HAIL_SOCKET` overrides tmux server detection (`TMUX_BRIDGE_SOCKET` still
  works). `hail doctor` explains why a server is not reachable.
- `hail --help` lists everything. `message`/`msg` are aliases of `send`;
  `tmux-bridge` is a symlink to `hail` for the alias week.

## Raw tmux (only when hail cannot do it)

```bash
tmux capture-pane -t %5 -p | tail -20          # last 20 lines of a pane
tmux send-keys -t %5 -l -- "text"              # type literally, no Enter
tmux send-keys -t %5 Enter                     # press a key
tmux split-window -h -t SESSION                # new pane (prefer over windows)
tmux select-layout -t SESSION tiled            # re-balance
tmux list-sessions; tmux new-session -d -s NAME; tmux kill-session -t NAME
```

Prefer `hail read`/`type`/`keys`: they resolve labels, verify typed text, and
keep the read guard honest.
