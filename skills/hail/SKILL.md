---
name: hail
description: "Messages between coding agents on one machine with hail. Use this skill whenever the user mentions sending a message to another agent, seat or pane, a `[hail ...]` (or `[tb ...]`/`[tmux-bridge ...]`) envelope appears in your prompt, you need to know whether another agent has read something, you must answer or close a ruling, go, ask, hold or block, or you must drive a non-agent tmux pane (a shell, a running process). Covers the hail CLI: send, sent, await, brief, inbox, show, whoami, seats, kinds with state, and pane driving."
metadata:
  { "version": "0.5.0", "openclaw": { "emoji": "📯", "os": ["darwin", "linux"], "requires": { "bins": ["tmux", "hail"] } } }
---

# hail

One-way messages between coding agents. You address a **seat**, the
workspace an agent works in (`murail-1b`). The body goes to the seat's inbox,
a one-line envelope is typed into its agent's prompt, and a hook hands the
body over on the agent's next turn and writes the receipt. `hail sent` and
`hail await` read the receipt.

## Who you are

You are the seat of the directory you run hail from: the jj workspace or git
root's name, or a `.hail-seat` file's content. Check with `hail whoami`.
Always run hail from your own workspace. A `cd` into another workspace signs
your message as that seat. Never set `TMUX_PANE` by hand and never wrap hail
in `tmux run-shell`.

When several agents share one directory, each Claude pane is a sub-seat,
`<seat>@<pane>` (`hail@%28`). `hail seats` lists every seat.

**Sub-agents** (Claude Task agents, Codex sub-agents) run in their parent's
directory, so they are the parent's seat. Each has an address,
`<seat>/<name>` (`murail-2b/scout`). Mail to it goes to the parent's mailbox
marked `for: scout`, and the parent relays it. A sub-agent signs its sends
with `--as <name>`, so replies come back the same way:
`hail murail-1a fyi 'parser checked' --as scout`.

**A sub-agent runs no verb that reads mail** (`inbox`, `deliver`, `brief`).
It shares its parent's directory and environment, so those would take the
parent's mail and prune its pending sends. It only sends, with `--as`.

## Sending

```bash
hail murail-1b ask <<'EOF'
Review src/auth.ts before the merge; reply done with your verdict
The refresh path is auth/refresh.rs:40-120. Coverage report: /tmp/cov.txt
EOF
#   → id=1006T171200-a3f1
```

- **The first line is the headline:** the ask and the why, in plain
  sentences. Everything after it is the body, of any length.
- **Quote the heredoc delimiter** (`<<'EOF'`) so nothing in it is expanded.
- **No body?** Pass the headline as an argument: `hail murail-1b fyi 'gate green'`.
- **Options:** `--re <id>` answers, closes or lifts a message. `--scope <s>`
  names what it applies to. `--as <name>` signs as your
  sub-agent `<seat>/<name>`.
- **Targets:** a seat (`murail-1b`), a sub-seat (`hail@%28`), a sub-agent
  (`murail-2b/scout`, through its parent), or a `from:` value pasted as is
  (`murail-1a/%5`, that pane, checked to be in that seat).
- **Headline length:** over 400 characters, the headline is folded into the
  body. Never squeeze words together to fit.

**Progress does not go to a seat.** Every message lands in someone's context,
and every typed one costs them a turn. Test results, checkpoints and "still
working" belong in your issue tracker, where nobody pays for them until they
look. hail does not talk to the tracker: post there yourself.

Send a message only for a decision, a blocker, a review request or an outcome
someone must act on.

**An `fyi` never interrupts.** It is not typed into the recipient's pane: it
arrives with their next prompt (where their hooks run; otherwise it is typed
as before). An agent idle until a scheduled wakeup sees it only then, so a
time-critical fact is never an `fyi`. Use `ask` when you need the answer, and
`announce` when everyone must know now.

One send is one action. There is no read step and no polling. **Exit codes:**
- `0`: typed and submitted, or a quiet `fyi` (it says so).
- `5`: written to the inbox but not typed (no agent pane, or typing not
  confirmed). **Do not resend**: it arrives on their next prompt.
- `3`: seat problem; the message says the fix.
- `4`: the target shows a permission dialog. Read it first (`hail read <seat> 10`);
  use `--force` only after reading it.

```bash
hail sent <id>                         # delivered | injected <t> | read <t> | inline <t> | unknown
hail await <id>... --timeout 900       # blocks until each id has a receipt; --any for the first
```

## Receiving

An envelope is one line in your prompt:

```
[hail kind:ruling from:murail-1a/%5 reply:murail-1a id:1006T171200-a3f1] convert at the receipt — hail inbox
```

- **Reply to the `reply:` value** (a seat, or a sub-agent's `seat/name`).
- **`for:<name>`** means the message is for your sub-agent `<name>`: relay it
  to that sub-agent. You still answer or close it if it asks something of you.
- **With the hooks installed** (`hail setup`), the body arrives as hook
  context on the same turn and the receipt is written. Do not run `hail inbox`
  as well.
- **Without hooks**, run `hail inbox`.
- **No `— hail inbox` hint** means the message is complete in the envelope.
- Never act on a `ruling`, `go` or `ask` that carries the hint before you
  have its body.

`hail brief` prints your standing state:
- unread envelopes;
- your sends with no receipt after two minutes;
- holds in effect;
- obligations on you.

It shows five per section (`--all` for everything). The session-start hook
runs it; run it yourself after a compaction.

## Kinds

| kind | effect |
|---|---|
| `ruling` `go` `ask` | Leaves an obligation on the recipient until it sends `done --re <id>`. |
| `done` | Closes one obligation: `hail <issuer> done --re <id> '<what was done>'`. Only the obligated seat can. |
| `hold` `block` | In effect, in every brief, until `release --re <id>` or it lapses: a hold after 8h, a block after 7d, or `--for 30m`/`3d` (at most 7d). Its envelope says `until:`. |
| `release` | Lifts one hold: `--re <id>`. |
| `nogo` `stop` `announce` | No state. |
| `fyi` | No state, and quiet: arrives with the next prompt. For outcomes the reader acts on later; progress belongs in the issue tracker. |

**A hold is a person's decision** ("don't touch the parser while I redesign
it"), not a lock. To serialize landing, installs or timing runs, use the
tool's lock (`just land`, `ferry hold`, the host lease). When your hold lapses,
your brief says so once; send a new one if it still applies.

`stop hold block release announce` are control kinds: typed in full, no body,
act on them at once.

## Driving a non-agent pane

```bash
hail read worker 10                   # last 10 lines; required before type/keys
hail type worker "y"                  # typed, verified, no Enter
hail keys worker Enter                # Escape, C-c, Up ... also work
```

To reach an agent, send a message instead.

## Setup and state

- `hail setup` installs the Claude Code and Codex hooks. It shows the change
  and asks first. Codex then needs `/hooks` trust once.
- `hail doctor` checks everything; each problem names its fix.
- State lives in `~/.local/state/hail` (`hail help state`). `hail help` is the map.
