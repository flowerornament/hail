# hail

Messages between coding agents — Claude Code and Codex — that share one
machine and one tmux server.

An agent sends a message to another agent's **seat** (its workspace). hail
writes the message to that seat's inbox and types a one-line envelope into the
recipient's prompt. The recipient's prompt hook puts the body in its context,
and that writes a receipt the sender can check or wait on. No daemon, no
database, no pane-reading.

```
[hail kind:ask from:murail-1a/%5 reply:murail-1a id:1006T171200-a3f1 bead:murail-ke7is] Review src/auth.ts against murail-ke7is; verdict on the bead — hail inbox
```

## Why

Agents working in parallel need to hand each other rulings, reviews, holds and
"done". Typing whole messages into each other's panes has three problems:

- **Context cost.** Codex keeps every user message verbatim through
  compaction, so a relayed message rides on every later API call. In one pane,
  relays took 36% of every request.
- **No delivery fact.** The sender has to read the other pane to learn whether
  the text landed or was answered, and every read costs a full-context call.
- **Fragile typing.** Long text cannot be verified as typed, and a terminal
  is not a transport: dialogs, drafts and scrolled panes eat keystrokes.

hail keeps the body out of the prompt, makes delivery a fact you can query,
and types only a short triage line.

## How it works

```
  sender (seat murail-1a)                          recipient (seat murail-1b)
  hail murail-1b ask <<'EOF'                         Codex or Claude Code, idle or busy
  <headline>                                      ┌───────────────────────────────────┐
  <body>                 (2) one-line envelope    │ [hail kind:ask from:murail-1a …]  │
  EOF            ───────────────────────────────▶ │ (typed, verified, Enter)          │
    │                                             └──────────────┬────────────────────┘
    │ (1) body written first                      (3) prompt hook: hail deliver
    ▼                                                            │ claims the body,
  ~/.local/state/hail/seats/murail-1b/new/<id>.md ───────────────┘ adds it to context
                                                  seats/murail-1b/cur/<id>.injected.md
  (4) hail sent <id>  → injected 2026-10-07T09:36:53Z     (the receipt)
      hail await <id> → blocks until there is one
```

1. **The body is written first.** It goes to the recipient's Maildir inbox,
   durably, before anything is typed.
2. **The envelope is typed.** It goes into the recipient agent's pane: hail
   leaves copy mode, refuses a pane showing a permission dialog, types,
   checks the text appeared, and presses Enter. If no agent is running there,
   the message waits in the inbox and the send exits 5.
3. **The hook delivers.** On the recipient's next prompt, its hook (`hail
   deliver`) claims the body by renaming the file, so exactly one reader
   wins, and adds it to the agent's context.
4. **The sender checks.** The claimed file is the receipt: `hail sent` and
   `hail await` read it, and nobody reads a pane.

### Seats

A seat is the workspace an agent works in, named after the directory. Going
up from the working directory:
1. the first `.hail-seat` file (its content is the name);
2. else the jj workspace root or git root, by its basename.

Identity comes from the directory only, never from the process tree or
`TMUX_PANE`. Codex runs every command under one shared daemon that carries
another pane's `TMUX_PANE`, and that misattributed messages in 0.3. Run hail
from your own workspace; `hail whoami` shows the seat.

When several agents share one directory, each Claude pane gets a **sub-seat**,
`<seat>@<pane>` (for example `hail@%28`). A send to the bare seat is refused
with the sub-seats listed. A Codex agent needs its own jj workspace, because
its commands cannot be told apart by pane.

### Kinds

| kind | what it does |
|---|---|
| `ruling` `go` `ask` | Leaves an obligation on the recipient, shown in its `brief` until it sends `done --re <id>`. |
| `done` | Closes one obligation. Only the obligated seat can. |
| `hold` `block` | In effect, and shown to the seats involved, until anyone sends `release --re <id>`. |
| `release` | Lifts a hold. |
| `fyi` `nogo` `stop` `announce` | No state. |

`stop`, `hold`, `block`, `release` and `announce` are control kinds: typed in
full, with no body, to act on at once.

## Install

### Nix with Home Manager

```nix
# flake inputs — the `release` branch always points at the latest tag
hail = { url = "github:flowerornament/hail?ref=refs/heads/release"; inputs.nixpkgs.follows = "nixpkgs"; };

# home-manager
imports = [ inputs.hail.homeManagerModules.default ];
programs.hail = {
  enable = true;
  skill.enable = true;                                      # link the agent skill
  skill.targets = [ ".agents/skills/hail" ".claude/skills/hail" ];
  # envelopeMax = 400;                                      # HAIL_ENVELOPE_MAX
};
```

The flake advertises the `flowerornament` Cachix cache. Builds for
`aarch64-darwin`, `aarch64-linux` and `x86_64-linux` are substituted rather
than compiled; accept the flake config, or add the substituter yourself.

### Other ways

- `nix build github:flowerornament/hail?ref=refs/heads/release`, or
  `nix run github:flowerornament/hail -- --help`.
- **A release tarball** from GitHub (aarch64 and x86_64, macOS and Linux). It
  holds `hail`, the `tmux-bridge` alias, and the skill under
  `share/hail/skills/hail`.
- `cargo install --path .` from a checkout. The pinned toolchain is in
  `rust-toolchain.toml`.

Requires tmux. `bd` (beads) is optional: a message that names an issue also
posts its body there as a comment.

### Hooks

Install them once, at user level. Every project picks them up, and nothing
is needed per repository:

```bash
hail setup     # shows the diff, asks, writes it (--yes to skip asking, --check to report drift)
hail doctor    # one line per check; every problem names its fix
```

`hail setup` edits these files:
- **Claude Code:** `~/.claude/settings.json`, plus `$CLAUDE_CONFIG_DIR/settings.json`
  when that is set to another directory.
- **Codex:** `~/.codex/config.toml`.

It replaces older hail lines in place, and keeps your other hooks and your
comments. The commands it installs:

| event | command |
|---|---|
| `SessionStart` | `hail brief --hook` |
| `UserPromptSubmit` | `hail deliver --format claude` (or `codex`) |

**Things to know about hooks:**
- **Codex trust:** Codex runs a changed hook only after you trust it with
  `/hooks`, in each session.
- **0.3 lines still work:** the 0.3 form (`[ -n "$TMUX_PANE" ] || exit 0;
  hail deliver --format codex`) works unchanged under 0.4, so you can leave it
  and keep its trust.
- **New sessions only:** both harnesses read hooks when a session starts.
- **Hooks are optional.** Without them the envelope still arrives, and the
  agent runs `hail inbox` for the body. Hooks never fail a session: errors go
  to `~/.local/state/hail/hook-errors.log`.

### Upgrading from 0.3

Run `hail migrate` once after upgrading:
- It moves the 0.3 inboxes, receipts, obligations and pending sends into the
  0.4 layout, and parks the old tree in `~/.local/state/hail/archive/0.3`.
- Recent unread mail keyed by a live pane id goes to that pane's seat.
  Everything else keyed by pane id is kept apart as `legacy-%N`.
- It lists unread mail that no agent will pick up.
- Until it runs, hooks are silent and other verbs ask for it. `hail migrate
  --revert` goes back.

Labels are gone: a seat is named by its directory, and `hail name` is a no-op.

## Use

```bash
hail whoami                                   # this directory's seat
hail seats                                    # every seat: agent panes, unread, obligations, root

# Send. The first line is the headline (the ask and the why); the rest is the body.
hail murail-1b ask --bead murail-ke7is <<'EOF'
Review src/auth.ts against murail-ke7is; verdict on the bead
The refresh path is auth/refresh.rs:40-120. Coverage: /tmp/cov.txt
EOF
#   id=1006T171200-a3f1
#   bead=murail-ke7is comment=7

hail murail-1b fyi 'gate green'               # headline only, no body
hail sent 1006T171200-a3f1                    # delivered | injected <t> | read <t> | inline <t> | unknown
hail await 1006T171200-a3f1 --timeout 900     # block until a receipt (--any for the first of several)

# Answer, close, lift.
hail murail-1a fyi --re 1006T171200-a3f1 '87% coverage; refresh path uncovered'
hail murail-1a done --re 1006T171200-a3f1 'committed on abc123'
hail murail-1b release --re 1006T165000-1c2e 'gate green'

hail brief                                    # what you were sent, what you owe, holds, late sends
hail inbox                                    # unread bodies (the hook usually delivers them)
hail show 1006T171200-a3f1                    # one message by id, from any seat
```

**How to send:**
- **Quote the heredoc delimiter** (`<<'EOF'`) so nothing in the body is
  expanded: backticks, `$()` and quotes arrive byte for byte.
- **Headline length:** a headline over 400 characters is folded into the
  body. A control kind over the cap is refused instead.
- **`--scope <s>`** says what a message applies to; `--no-submit` types
  without pressing Enter.
- **`--force`** skips the dialog guard. Use it only after `hail read <seat> 10`.

**Exit codes:**

| code | meaning |
|---|---|
| 0 | typed and submitted |
| 1 | usage or state error; the message says what to run |
| 2 | control-kind headline over the cap |
| 3 | seat problem: no seat here, a shared seat addressed bare, or a name bound elsewhere |
| 4 | the target shows a permission dialog |
| 5 | in the inbox but not typed (no agent running, or typing not confirmed). Do not resend: it arrives on their next prompt |

`read`, `type` and `keys` drive panes that are not agents: a shell, a gate
run, a prompt waiting for `y`. Read before typing; the guard enforces it.

`hail help` is the map, and `hail help <topic>` has the details: `send`,
`kinds`, `receive`, `seats`, `panes`, `setup`, `state`.

### For agents

`skills/hail/SKILL.md` is the instruction set agents load. With the Home
Manager module's `skill.enable`, it is linked where Claude Code and Codex
discover skills.

## Troubleshooting

Start with `hail doctor`. It checks:
- the seat for this directory and its root;
- tmux, the agent panes, and this seat's wake pane or sub-seats;
- the hooks in each harness config;
- unread mail waiting in seats where no agent sits;
- whether the state is due for `hail gc`.

| symptom | cause and fix |
|---|---|
| A send exits 5 | Written to the inbox, not typed: no agent runs in that seat, or the text did not show in 2 s. It arrives on the recipient's next prompt (or its next session's brief). Do not resend. |
| `sent` stays `delivered` | The recipient's hook has not run since. Either it has not had a prompt yet, or its hooks are not installed or trusted (`hail doctor`; in Codex, `/hooks`). |
| A send exits 3 "has N agents" | Several agents share that directory. Address one sub-seat, as listed, or give each agent its own jj workspace. |
| A send exits 3 "no seat here" | You are outside a workspace. `cd` to it, or add a `.hail-seat` file naming it. |
| A send exits 4 | The target shows a permission dialog. Read it (`hail read <seat> 10`), resolve it, and resend. |
| Nothing arrives in a session | The hooks are read at session start, so start a new session after `hail setup`. Check `~/.local/state/hail/hook-errors.log`. |
| The brief lists "unread mail waits" in an old seat | Mail sent to a name no agent uses any more. Read it with `hail show <id>`. |

## State and environment

Everything lives under `$XDG_STATE_HOME/hail` (default
`~/.local/state/hail`), outside every repository:

| path | holds |
|---|---|
| `seats/<seat>/new/<id>.md` | unread messages |
| `seats/<seat>/cur/<id>.<how>.md` | claimed (`injected`, `read` or `inline`); the mtime is the receipt time |
| `seats/<seat>/owed/<id>` | obligations on the seat |
| `seats/<seat>/pending/<id>` | its sends with no receipt yet |
| `holds/<id>` | holds and blocks in effect; delete one to lift it silently |
| `ids/<id>` | the id index (a symlink to the seat) |
| `archive/` | `hail gc` output and the 0.3 tree |

Read mail is kept until `hail gc` archives it (by default, mail older than
90 days); `doctor` says when gc is due. Each prompt gets at most five bodies,
about 8 KB, and the rest arrive on later prompts. The brief shows five
entries per section (`--all` for everything) and a send without a receipt
after two minutes. It drops a send that has lapsed after seven days.

| variable | default | what it does |
|---|---|---|
| `HAIL_ENVELOPE_MAX` | 400 | headline cap in characters |
| `HAIL_SOCKET` | `$TMUX`, else the default server | tmux socket (`TMUX_BRIDGE_SOCKET` is accepted too) |
| `HAIL_SEAT` | — | a seat name, used only where the directory names none |
| `HAIL_AGENT_COMMANDS` | `claude,codex` | commands that count as agents |
| `XDG_STATE_HOME` | `~/.local/state` | the state root |

Hooks and receipt checks touch only the filesystem: about 2.5 ms of CPU each,
with no subprocess. A send's own work takes under 20 ms, plus a 300 ms pause
before Enter so the agent's composer does not treat the text as a paste.

## Compatibility

- `tmux-bridge` is installed as an alias of `hail`, and `message` and `msg`
  are aliases of `send`.
- The 0.3 send form still works: `hail <target> '<headline>' --kind <k> [--body text|file|-]`.
- `name`, `hello`, `who`, `resolve` and `id` remain as shims through 0.4.
- Envelopes tagged `[tb …]` or `[tmux-bridge …]` mean the same as `[hail …]`.

## Develop

```bash
just check     # fmt, clippy, unit and integration tests, release-script tests, tmux scenarios, CPU bench
just build     # the Nix package
just land      # gate the described jj change, then move main to it and push
```

**The gate:**
- **Lints:** clippy runs pedantic, with `unwrap` and `expect` denied.
- **Scenarios:** `test/run.sh` runs 48 scenarios on a scratch tmux server and
  a scratch state root, so it never touches yours. It needs bash 4 or later.
- **On a loaded machine,** set `HAIL_TEST_DELIVER_MS=100`.
- **CI** runs the gate on Linux and macOS with the toolchain pinned in
  `rust-toolchain.toml`.

Work happens in jj, with one workspace per agent, and `just land` is the only way to publish. [AGENTS.md](AGENTS.md) has that workflow and the rules for working on hail;
[DESIGN.md](DESIGN.md) has the principles; the 0.4 spec and studies are in
[docs/](docs/); the hook blocks for a hand install are in
[hooks/README.md](hooks/README.md).

### Release

The version lives in one place, `Cargo.toml`.

```bash
# in a jj workspace
just release-bump 0.4.1   # set Cargo.toml and Cargo.lock; scaffold the CHANGELOG entry
$EDITOR CHANGELOG.md      # replace the TODO bullet with what changed
jj describe -m "0.4.1" && just land
                          # wait for the Nix Cache workflow on that commit to succeed
# in the colocated checkout, ~/code/hail
jj git fetch && jj new main
just release-verify       # versions agree, changelog filled, clean tree, the gate, Nix build
just release-tag 0.4.1    # checks every system's build is in Cachix, tags, moves origin/release
```

Every push to `main` builds the package for each system, pushes it to Cachix
and proves it substitutes. The tag publishes a GitHub release with that
CHANGELOG section as its notes and binaries attached. Consumers that track the
`release` branch pick it up with `nix flake update hail` (or `nx upgrade
hail`).
