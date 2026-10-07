# hail

Messages between coding agents (Claude Code, Codex) that share a machine and
a tmux server.

You address a **seat**: the workspace an agent works in. hail writes the
body to the seat's inbox, types a one-line envelope into its agent's prompt,
hands the body over through the agent's prompt hook, and records a receipt
the sender can query.

```
[hail kind:ruling from:murail-1a/%5 reply:murail-1a id:1006T171200-a3f1 bead:murail-ke7is] convert at the receipt, not the producer — hail inbox
```

- **Seat:** the jj workspace or git root a process runs in, by name, or the
  content of a `.hail-seat` file. It comes from the working directory, never
  from the process tree or `TMUX_PANE`. Several agents in one directory are
  told apart as `<seat>@<pane>`.
- **Envelope** in the prompt: kind, sender, reply seat, id, and optionally a
  bead, `re:` and `scope:`, then the headline. The headline is the ask and the
  why, at most 400 characters; a longer one folds into the body.
- **Body** in the seat's inbox (a Maildir). The `UserPromptSubmit` hook
  claims it, puts it in the agent's context and writes the receipt. Exactly
  one claimer wins.
- **Receipts:** `injected <time>`, `read <time>` or `inline <time>`.
  `hail sent` and `hail await` read them.
- **State:** `ruling`, `go` and `ask` leave obligations closed by `done`;
  `hold` and `block` stay in effect until `release`. `hail brief` lists them,
  bounded.
- No daemon, no database. State is files under `~/.local/state/hail`.
- One Rust binary. Hooks and receipt checks run in a few milliseconds and
  start no other process.

## Install

Nix flake with a Home Manager module:

```nix
# flake.nix inputs — the `release` branch always points at the latest tag
hail = { url = "github:flowerornament/hail?ref=refs/heads/release"; inputs.nixpkgs.follows = "nixpkgs"; };

# home-manager
imports = [ inputs.hail.homeManagerModules.default ];
programs.hail = {
  enable = true;
  skill.enable = true;                       # links skills/hail into the paths below
  skill.targets = [ ".agents/skills/hail" ".claude/skills/hail" ];
};
```

Or `nix build .#` and put `result/bin/hail` on your PATH, or `cargo install
--path .`. Each GitHub release also attaches binaries for macOS and Linux. Requires `tmux`.
`bd` (beads) is optional: when a message names an issue, the body is also
posted there.

Then install the hooks once:

```bash
hail setup        # shows the change to ~/.claude/settings.json and ~/.codex/config.toml, asks, applies
hail doctor       # checks everything; each problem names its fix
```

In Codex, run `/hooks` once to trust them. `hooks/README.md` has the blocks
for doing it by hand. Without hooks everything still works: the envelope
lands in the prompt, `hail inbox` fetches bodies, and `hail brief` shows the
standing state.

### Upgrading from 0.3

Run `hail migrate` once after the upgrade:
- It moves the 0.3 inboxes, receipts, obligations and pending sends into the
  0.4 layout. Mail keyed by a live pane id goes to that pane's seat.
- It parks the old tree in `~/.local/state/hail/archive/0.3`.
- Until it runs, hooks stay silent and other verbs ask for it.
- `hail migrate --revert` goes back.

Labels are gone: a seat is named by its directory, and `hail name` is a no-op.

## Use

```bash
hail whoami                                 # this directory's seat
hail seats                                  # every seat: agent panes, unread, open obligations

hail murail-1b ask --bead murail-ke7is <<'EOF'
Review src/auth.ts against murail-ke7is; verdict on the bead
The refresh path is auth/refresh.rs:40-120.
EOF
#   id=1006T171200-a3f1  bead=murail-ke7is comment=7

hail murail-1b fyi 'gate green'             # headline only
hail sent 1006T171200-a3f1                  # delivered | injected <t> | read <t> | inline <t> | unknown
hail await 1006T171200-a3f1 --timeout 900   # block until a receipt; --any for several ids
hail murail-1a done --re 1006T171200-a3f1 'committed on abc123'
hail brief                                  # unread, sends without receipt, holds, obligations
```

- **Exit codes:** 0 typed and submitted; 5 in the inbox but not typed (do
  not resend); 3 seat problem; 4 the target shows a permission dialog;
  2 control-kind headline over the cap.
- **Panes:** `read`, `type` and `keys` drive non-agent panes: a shell, a gate
  run, a prompt waiting for `y`.
- **Help:** `hail help` is the map, and `hail help <topic>` has the details.

### For agents

`skills/hail/SKILL.md` is the agent-facing instruction set. With the module's
`skill.enable`, it is linked where Claude Code and Codex discover skills.

### Compatibility

- `tmux-bridge` is installed as an alias of `hail`.
- `message` and `msg` are aliases of `send`.
- The 0.3 send form `hail <target> '<headline>' --kind k --body X` still works.
- Envelopes tagged `[tb …]` or `[tmux-bridge …]` mean the same as `[hail …]`.

## Develop and release

`just check` runs:
- `cargo fmt --check` and clippy;
- the unit and integration tests;
- the release-script tests;
- the scenario harness (`test/run.sh`, on a scratch tmux server and state
  root that never touch yours);
- the speed checks (`scripts/bench.sh`).

`just build` builds the Nix package. CI runs the gate on Linux and macOS with
the toolchain pinned in `rust-toolchain.toml` (Rust 1.99.0; the crate builds
with 1.98, nixpkgs' rustc). Every push to `main` publishes the Nix package for
four systems to the `flowerornament` Cachix cache (the flake advertises it),
so `nx upgrade hail` substitutes rather than compiles.

Design: [DESIGN.md](DESIGN.md) and the 0.4 spec in [docs/](docs/). Hook
blocks: [hooks/README.md](hooks/README.md).

The version lives in one place, `Cargo.toml`. To cut a release:

```
just release-bump 0.4.1      # sets Cargo.toml and Cargo.lock, scaffolds a CHANGELOG.md entry
$EDITOR CHANGELOG.md         # replace the TODO bullet with what changed
git commit -am "0.4.1" && git push
just release-verify          # versions agree, changelog filled, checks, Nix build
just release-tag 0.4.1       # checks the cache has every system's build, tags v0.4.1, moves origin/release
```

Pushing the tag publishes a GitHub release with that CHANGELOG section as its
notes and binaries for aarch64/x86_64 macOS and Linux. Consumers that track the `release` branch pick it up with
`nix flake update hail` (or `nx upgrade hail`).
