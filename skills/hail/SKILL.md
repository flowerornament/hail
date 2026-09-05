---
name: hail
description: "Agent-to-agent messaging over tmux with hail. Use this skill whenever the user mentions sending a message to another agent or pane, a `[hail ...]` (or `[tb ...]`/`[tmux-bridge ...]`) envelope appears in your prompt, you need to know whether another agent has read something, you must answer or close a ruling, go, ask, hold or block, or you must drive a non-agent tmux pane (a shell, a running process). Includes the hail CLI: send, deliver, brief, inbox, sent, await, kinds with state, labels, and a minimal raw-tmux fallback."
metadata:
  { "openclaw": { "emoji": "📯", "os": ["darwin", "linux"], "requires": { "bins": ["tmux", "hail"] } } }
---

# hail

One-way messages between coding agents that share a machine. The body goes to
a file; a one-line envelope goes into the other agent's prompt; the hook
delivers the body on the recipient's next turn and writes the receipt;
`hail sent` / `hail await` read it.

## Receiving

An envelope is one line in your prompt:

```
[hail kind:ruling from:murail-1a/%5 reply:%5 id:0905T1712-a3f1 bead:murail-ke7is re:0905T1650-1c2e scope:commit] convert at the receipt — hail inbox
```

`kind` says what to do (see Kinds). `from` is the sender's label and pane; reply to
the `reply:` pane id or the label alone, never to `label/%N`. `id`
is for receipts and `--re`. `bead` is the issue the body is also posted to.
`re` names the message this answers, closes or lifts. `scope` is what it
applies to. The headline is written by the sender and is never truncated.

- **With the hooks installed** (see below) the body arrives with the envelope
  as hook context on the same turn, and the receipt is written for you. Read
  it there; do not run `hail inbox` as well. It is not retained through compaction; the file and the bead are.
- **Without the hooks**, or if the body did not arrive, run `hail inbox`. It
  prints every unread body and writes the receipt. `hail inbox --all`
  re-reads everything, with each message's receipt (`read` or `injected`).
- A message sent with no `--body` is complete in the envelope: no `— hail
  inbox` hint, and the hook injects nothing for it (the receipt is still
  written). Act on the headline. With `--body`, the hint is present and the
  body follows.
- Never act on a `ruling`, `go` or `ask` from the envelope alone when it
  carries the hint.
- `[tb ...]` and `[tmux-bridge ...]` envelopes mean the same as `[hail ...]`.

`hail brief` prints your standing state: unread envelopes, your sends with no
receipt after two minutes, holds and blocks in effect, and open obligations on
you. It prints nothing when there is nothing. The SessionStart hook runs it;
run it yourself after a compaction or when unsure what you owe.

## Kinds

| kind | effect |
|---|---|
| `ruling` `go` `ask` | Creates an obligation on the recipient, listed in its brief until it sends `done --re <id>`. |
| `done` | Closes one obligation. `--re <id>` required; `--scope` names the part done. Only the obligated party can close it. |
| `nogo` `fyi` | No state. `fyi` for status; `ask` only when you expect a reply. |
| `hold` `block` | Records a hold in effect (`--scope` says on what). Listed in every brief until released. |
| `release` | Lifts one hold or block: `--re <id>`. Refused for anyone but its issuer. |
| `stop` `announce` | No state. |

`stop hold block release announce` are control kinds: typed in full, no fetch
hint, act on them at once. The others carry the body through the hook or
`hail inbox`. `--kind` is required. Every headline is capped at
`HAIL_ENVELOPE_MAX` characters (default 400); a longer one is refused (exit 2).
Put detail in `--body` or on a bead.

## Sending

```bash
hail read <target> 5                                   # read guard
hail <target> '<headline>' --kind <kind> [--bead id] [--re id] [--scope s] [--body file|-]
#   → id=0905T1712-a3f1            (and bead=<id> comment=<n> when posted)
```

`send` presses Enter for you. Read, send, done. `--no-submit` types without
Enter. When the target runs a shell (bash, zsh, sh, fish) `send` types the
envelope but does not press Enter, because the shell would execute it;
`--force` submits anyway. Non-agent panes are driven with `type`/`keys`. Before typing, `send` refuses a pane that shows a permission dialog
(exit 4: on Claude Code the text is discarded and Enter approves the command)
or an unsent draft that is not an envelope (exit 5); `--force` overrides.
`hail send`, `message` and `msg` are accepted before the target.

```bash
hail sent <id>                     # delivered | read <time> | injected <time> | unknown
hail await <id>... [--timeout SECONDS] [--any]
#   blocks until every id (or any) has a receipt; one line per id; exit 1 on timeout
```

Typical exchange:

```bash
hail read codex 5
id=$(hail codex 'Review src/auth.ts against murail-ke7is; verdict on the bead' --kind ask)
id=${id#id=}
hail await "$id" --timeout 900      # returns when codex's hook injected the body
```

Answering, closing, lifting:

```bash
hail read murail-1a 5
hail murail-1a '87% line coverage; OAuth refresh uncovered' --kind fyi --re 0905T1712-a3f1
hail murail-1a 'committed on base abc123' --kind done --re 0905T1712-a3f1 --scope commit
hail murail-1b 'gate green' --kind release --re 0905T1650-1c2e
```

Rules:

- A receipt means the body was injected into the recipient's context, not
  that it was attended to. Obligations stay in the brief until `done`.
- Do not read an agent pane to check delivery or look for a reply; `sent`,
  `await` and your own inbox tell you. `await` is the only verb that waits.
- One recipient per message; loop for several.
- `--bead <id>` (or an issue id in the headline) posts the body as a bead
  comment. If `bd` is missing or fails: one warning, message goes file-only.
- `--re` when a message answers, closes or lifts another. `--scope` on `go`
  names the permitted action (`commit`, `push`) and the base; on `hold` the
  thing held; on `done` the part completed.

## Identity

```bash
hail name "$(hail id)" murail-1b      # label this pane; mints its incarnation and registers both
hail hello                            # print the incarnation; creates one only if the pane has none
hail list                             # every pane with its label
hail who [label]                      # pane, label, incarnation, last inbox event, last 2 pane lines
```

Inboxes and `from:` are keyed on labels. `send` resolves a label through its
registration and refuses (exit 3, `label X moved: registered on %N, now on
%M — run hail name to re-register`) when a different pane now wears the label
or the registered pane's process was restarted. The brief then prints
`label <l>: pane restarted — run: hail name "$(hail id)" <l>`; `hail who
<label>` shows both sides.

## Hooks

The hooks deliver bodies and the brief without a tool call. They are
installed once at user level (`hooks/README.md` in the hail repo has the
blocks; per-project overrides are optional):

- Claude Code `~/.claude/settings.json`: `UserPromptSubmit` →
  `hail deliver --format claude`; `SessionStart` → `hail brief`.
- Codex `~/.codex/config.toml`: `[[hooks.UserPromptSubmit]]` →
  `hail deliver --format codex`; `[[hooks.SessionStart]]` → `hail brief`.
  Trust them once with `/hooks`.

Without hooks: the envelope still lands in your prompt, `hail inbox` fetches
bodies and writes `read` receipts, and `hail brief` on demand shows the
standing state.

Hooks are read at session start; a new session picks up an install.

## Non-agent panes: read, type, keys

For a plain shell or a running process you drive the pane and read its
output. `read` marks the pane; `type`, `keys` and `send` require the mark and
consume it.

```bash
hail read worker 10
hail type worker "y"                  # verified, no Enter
hail keys worker Enter                # Escape, C-c, C-d, Up ... also work
hail read worker 20
```

`type` cannot verify text past ~400 characters; put long content on a bead or
in `--body`.

## State and environment

- `$XDG_STATE_HOME/hail` (default `~/.local/state/hail`): `inbox/<label>/<id>.md`
  and `<id>.read`, `obligations/<label>/<id>`, `holds/<id>`, `identity/<label>`,
  `incarnation/<pane>`, `sent/<label>/<id>`. Keep it outside every checkout.
- `HAIL_SOCKET` overrides tmux server detection (`TMUX_BRIDGE_SOCKET` also
  works). `hail doctor` reports why a server is unreachable.
- Exit codes: 1 usage/state, 2 headline over cap, 3 label moved, 4 permission
  dialog in target, 5 unsent draft in target.
- `hail --help` lists everything.

## Raw tmux (only when hail cannot do it)

```bash
tmux capture-pane -t %5 -p | tail -20          # last 20 lines of a pane
tmux send-keys -t %5 -l -- "text"              # type literally, no Enter
tmux send-keys -t %5 Enter                     # press a key
tmux split-window -h -t SESSION                # new pane
tmux select-layout -t SESSION tiled            # re-balance
tmux list-sessions; tmux new-session -d -s NAME; tmux kill-session -t NAME
```

Prefer `hail read`/`type`/`keys`: they resolve labels, verify typed text and
keep the read guard.
