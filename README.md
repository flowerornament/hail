# hail

Messaging between coding agents that share a machine and a tmux server.

Agents in different panes (Claude Code, Codex, anything with a prompt) need to
hand each other rulings, permissions, stops and results. Typing a whole message
into another agent's prompt makes it part of that agent's context for the life
of the session. hail types a one-line envelope instead, keeps the body in a file,
and tells the sender when the recipient has read it.

```
[hail ruling from:murail-1a id:0905T1712-a3f1 bead:murail-ke7is] convert at the receipt, not the producer — hail inbox
```

- **Envelope** in the pane: kind, sender, id, optional bead, a headline you wrote.
- **Body** on disk, fetched by the recipient with one command.
- **Receipt** written by that fetch. The sender checks or waits on it; nobody
  reads another pane to see whether a message landed.
- **Control kinds** (`stop`, `hold`, `nogo`, `announce`) are typed in full, so
  they work even if nothing is fetched.
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
`fswatch` is optional; `await` polls without it. `bd` (beads) is optional; when a
message names an issue id the body is also posted there.

## Use

Label your pane once. Inboxes and envelopes use labels, not pane ids.

```bash
hail name "$(hail id)" murail-1b
hail list                                   # every pane, with labels
```

Send. Read the target first (the read guard), then send, then submit.

```bash
hail read murail-1b 5
hail send murail-1b 'convert at the receipt, not the producer' --kind ruling --bead murail-ke7is --body ruling.md
#   id=0905T1712-a3f1  bead=murail-ke7is comment=7
hail keys murail-1b Enter
```

Receive. When an envelope appears in your prompt:

```bash
hail inbox                                  # print unread bodies, write receipts
hail inbox --peek                           # look without marking read
```

Confirm delivery without reading the other pane:

```bash
hail sent 0905T1712-a3f1                    # delivered | read <time> | unknown
hail await 0905T1712-a3f1 --timeout 600     # block until read; --any for several ids
```

Kinds: `ruling go nogo ask fyi stop hold announce`. Default is `ask`.
The headline is capped at `HAIL_ENVELOPE_MAX` characters (default 400); the
body has no limit. `hail --help` documents every verb and flag.

`read`, `type` and `keys` still drive non-agent panes: a shell, a gate run, a
prompt waiting for `y`.

## For agents

`skills/hail/SKILL.md` is the agent-facing instruction set. With the module's
`skill.enable`, it is linked where Claude Code and Codex discover skills.

## Compatibility

`tmux-bridge` is installed as an alias of `hail` and `message` as an alias of
`send`, for one week after switching from smux. Envelopes tagged `[tb …]` or
`[tmux-bridge …]` mean the same as `[hail …]`.

## Development

```bash
test/run.sh         # 20 scenarios on a scratch tmux server; never touches yours
bash -n bin/hail && shellcheck bin/hail
```

Design, rationale and the reviewed roadmap: [DESIGN.md](DESIGN.md).
