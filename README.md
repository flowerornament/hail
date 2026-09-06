# hail

Messaging between coding agents that share a machine and a tmux server.

hail types a one-line envelope into the recipient's pane, keeps the body in a
file, delivers the body through the harness's prompt hook, and records a
receipt the sender can query.

```
[hail kind:ruling from:murail-1a/%5 reply:%5 id:0905T1712-a3f1 bead:murail-ke7is scope:commit] convert at the receipt, not the producer — hail inbox
```

- **Envelope** in the pane: kind, sender, id, optional bead, `re:`, `scope:`,
  and a headline the sender wrote. Never truncated; over the cap is refused.
- **Body** on disk, injected into the recipient's context by the
  `UserPromptSubmit` hook (`hail deliver`), or fetched with `hail inbox`.
- **Receipt** written by that delivery: `injected <time>` or `read <time>`.
  `hail sent` and `hail await` read it.
- **State**: `ruling`, `go` and `ask` create obligations closed by `done`;
  `hold` and `block` stay in effect until `release`. `hail brief` lists them.
- **Identity**: labels are registered to a pane and its incarnation; a send
  to a label that moved is refused.
- **Control kinds** (`stop`, `hold`, `block`, `release`, `announce`) are typed
  in full.
- No daemon, no database. State is files under `~/.local/state/hail`.

## Install

Nix flake with a home-manager module:

```nix
# flake.nix inputs
hail = { url = "git+ssh://git@github.com/flowerornament/hail"; inputs.nixpkgs.follows = "nixpkgs"; };

# home-manager
imports = [ inputs.hail.homeManagerModules.default ];
programs.hail = {
  enable = true;
  skill.enable = true;                       # links skills/hail into the paths below
  skill.targets = [ ".agents/skills/hail" ".claude/skills/hail" ];
  # envelopeMax = 400;                       # HAIL_ENVELOPE_MAX
};
```

Or `nix build .#` and put `result/bin/hail` on your PATH. Requires `tmux`.
`fswatch` is optional; `await` polls without it. `bd` (beads) is optional; when
a message names an issue id the body is also posted there.

Install the hooks once at user level: `hooks/README.md` has the blocks for
`~/.claude/settings.json` and `~/.codex/config.toml` (Codex needs `/hooks`
trust once), plus per-project overrides. Without hooks everything still
works: the envelope lands in the prompt, `hail inbox` fetches bodies and
writes `read` receipts, and `hail brief` on demand shows the standing state.

## Use

Label your pane once. Inboxes, envelopes and identity use labels.

```bash
hail name "$(hail id)" murail-1b
hail list                                   # every pane, with labels
hail who murail-1a                          # pane, label, incarnation, last event, last 2 lines
```

Send: read the target, then send. `send` types the envelope, verifies it and
presses Enter.

```bash
hail read murail-1b 5
hail murail-1b 'convert at the receipt, not the producer' --kind ruling --bead murail-ke7is --body ruling.md
#   id=0905T1712-a3f1  bead=murail-ke7is comment=7
```

`send`, `message` and `msg` are accepted before the target. `--re <id>` names
the message this answers, closes or lifts; `--scope` says what it applies to;
`--no-submit` skips Enter; `--force` skips the dialog and draft guards. A
message with no `--body` is complete in the envelope: no `— hail inbox` hint
and nothing injected by the hook; the receipt is still written.

Receive: with the hooks installed the body arrives with the envelope. Otherwise:

```bash
hail inbox                                  # print unread bodies, write receipts
hail inbox --peek                           # look without marking read
hail brief                                  # unread, sends without receipt, holds, obligations
```

Confirm delivery:

```bash
hail sent 0905T1712-a3f1                    # delivered | read <time> | injected <time> | unknown
hail await 0905T1712-a3f1 --timeout 600     # block until a receipt; --any for several ids
```

Close and lift:

```bash
hail murail-1a 'committed on base abc123' --kind done --re 0905T1712-a3f1 --scope commit
hail murail-1b 'gate green' --kind release --re 0905T1650-1c2e
```

Kinds: `ruling go nogo ask fyi done stop hold block release announce`. `--kind`
is required. The headline is capped at `HAIL_ENVELOPE_MAX` characters (default
400); the body has no limit. `--body` takes text, a file, or `-` for stdin. Exit codes: 2 headline over cap, 3 label moved,
4 permission dialog in the target, 5 unsent draft in the target.
`hail --help` documents every verb and flag.

`read`, `type` and `keys` drive non-agent panes: a shell, a gate run, a prompt
waiting for `y`.

## For agents

`skills/hail/SKILL.md` is the agent-facing instruction set. With the module's
`skill.enable`, it is linked where Claude Code and Codex discover skills.

## Compatibility

`tmux-bridge` is installed as an alias of `hail` and `message` as an alias of
`send`. Envelopes tagged `[tb …]` or `[tmux-bridge …]` mean the same as
`[hail …]`.

## Development

```bash
test/run.sh         # 35 scenarios on a scratch tmux server; never touches yours
bash -n bin/hail && shellcheck bin/hail
```

Design and rationale: [DESIGN.md](DESIGN.md). Hook snippets: [hooks/README.md](hooks/README.md).
