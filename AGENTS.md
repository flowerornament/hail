# hail agent guide

hail carries messages between coding agents on one machine: one Rust binary,
a Maildir store, and tmux to wake the recipient. Agents run it on every
prompt through hooks, so a bug here breaks every other session on the
machine. Priorities, in order: delivery is correct, hot paths are fast, and
the code is easy for the next agent to maintain.

Where things are: the behaviour contract is
`docs/2026-10-06-rust-port-spec.md`, the design and module map are in
`DESIGN.md`, and what agents are told is in `skills/hail/SKILL.md` and
`src/help.rs`.

## Rules, and why

- **`just check` is the gate:** fmt, clippy (pedantic), the tests, the
  scenarios in `test/run.sh` on a scratch tmux server, and the CPU bench.
  jj runs no git hooks, so the gate runs only when `just land` runs it. On a
  loaded machine, set `HAIL_TEST_DELIVER_MS=100`.
- **Never point hail or its tests at the real `~/.local/state/hail`.** It
  holds every agent's live mail. Use a scratch `XDG_STATE_HOME` and `HOME`.
- **Identity comes only from the working directory.** Never read the process
  tree, and never trust `TMUX_PANE` for who a process is. Codex runs every
  command from one shared daemon, so its `TMUX_PANE` names whichever pane
  started that daemon. 0.3 misrouted mail this way.
- **Hot paths start no subprocess:** `deliver`, `brief`, `sent`, `show`,
  `whoami`. Hooks run on every prompt, and one fork costs more than the whole
  budget.
- **Hooks never fail a session.** They exit 0 and log to
  `~/.local/state/hail/hook-errors.log`. Hail is an add-on, so it must never
  break the agent it serves.
- **No `unsafe`, and no `unwrap` or `expect` outside tests.** Agents read the
  error text and the exit code, not a panic: each `error::Error` carries its
  exit code and ends with the command to run.
- **Keep pure cores pure:** `envelope`, `store::message`,
  `transport::dialog`, `hooks::setup` and `seat::seat_of` do no I/O and keep
  their tests beside them. They can then be tested without tmux.
- **A behaviour change updates the spec, `DESIGN.md`, the skill,
  `src/help.rs` and `CHANGELOG.md` together.** Agents learn hail from the
  skill and the help pages, so an out-of-date page misleads every one of them.
- **Comments state the rule, not the history.** A reader a year from now
  won't know what 0.3 was. Put the history in the CHANGELOG.
- **Track work in bd** (`bd ready`, `bd create`, `bd close`), and put the id
  in the change description.

Toolchain: Rust 1.99.0 is pinned in `rust-toolchain.toml`. `rust-version`
is 1.98 because the Nix package builds with nixpkgs' rustc. Add dependencies
with `cargo add`.

## Version control: jj, one workspace per agent

```text
~/code/hail      colocated checkout (.git + .jj): bd home and releases.
                 No agent edits code here.
~/code/hail-1a   jj workspace: coordinator (reviews, gives GO)
~/code/hail-1b   jj workspace: implementer
```

Each agent gets its own workspace, so it never snapshots another agent's
half-finished edits. hail names a seat after the jj root, so each workspace
is also a hail address: `hail hail-1b ask '…'` reaches the agent working
there.

- **Work:** `jj new main`, edit, then `jj describe -m "area: subject
  (hail-xxxx)"`. To pick up others' work, run `jj git fetch` and then
  `jj rebase -d main`.
- **Publish:** `just land`. It runs the gate on your change, together with
  any described commits beneath it, and only then moves `main` and pushes.
  A raw `jj git push` skips the gate, so never run one. The implementer
  lands after the coordinator's GO.
- **Trailers:** a SessionStart hook (`scripts/jj-identity`) adds
  `Jj-Workspace:`, `Agent:` and `Session:` to every description. Don't type
  or strip them.
- **Git:** run it only in `~/code/hail`, and only to read. A workspace has
  no `.git`, and `~/.git` exists, so git in a workspace silently answers
  about your home directory.
- **Releases:** run them from `~/code/hail` after `jj git fetch && jj new
  main`; `scripts/release.py` refuses to run anywhere else. Bump in a
  workspace with `just release-bump X.Y.Z`, fill in the CHANGELOG, and
  `just land`. Wait for the Nix Cache workflow, then run `just
  release-verify` and `just release-tag X.Y.Z` (README, "Release").
- **Stale workspace:** when another agent rewrites a commit under your `@`,
  jj refuses until you run `jj workspace update-stale`. Never `jj edit` a
  change that is an ancestor of another workspace's `@`.
- **Recovery:** start with `jj op log`, `jj undo` or `jj op restore`, before
  any destructive file operation. The `jj-ops` skill has the full model.
- **New workspace:** run `just workspace-add hail-<pair><letter>` in
  `~/code/hail`. It also points bd and Claude's memory at the main
  checkout. `jj workspace list` is the roster.

## Done means landed

1. `jj describe -m "area: subject (hail-xxxx)"`, then `just land`.
2. `bd close <id>`, then `bd dolt push`.
