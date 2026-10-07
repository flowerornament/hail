# hail agent guide

hail carries messages between coding agents on one machine. It is one Rust
binary. The behaviour contract is `docs/2026-10-06-rust-port-spec.md`; the
design is in `DESIGN.md`; the agent-facing instructions are in
`skills/hail/SKILL.md`.

Priorities: correctness of delivery, speed of the hot paths (hooks run on
every prompt), and code a different agent can maintain next week.

## Rules

- `just check` is the gate: fmt, clippy (the nx-rs lint set, pedantic),
  unit and integration tests, release-script tests, the tmux scenarios
  (`test/run.sh`, on a scratch server and state root) and the CPU bench. On a
  loaded machine, set `HAIL_TEST_DELIVER_MS=100`.
- Pure cores, I/O at the edges: `envelope`, `store::message`,
  `transport::dialog`, `hooks::setup` and `seat::seat_of` have no process or
  tmux access, and carry their tests beside the code.
- No `unsafe`; no `unwrap`/`expect` outside tests. Errors carry their exit
  code (`error::Error`) and end with what to run.
- Hot paths (`deliver`, `brief`, `sent`, `show`, `whoami`) start no
  subprocess. Hooks never fail a session: they log to
  `~/.local/state/hail/hook-errors.log`.
- Identity comes from the working directory only. Never read the process
  tree or trust `TMUX_PANE` for who a process is.
- Never run `hail migrate` or tests against the real
  `~/.local/state/hail`; use a scratch `XDG_STATE_HOME` and `HOME`.
- A behaviour change updates the spec, `DESIGN.md`, the skill, the help
  pages (`src/help.rs`) and `CHANGELOG.md` together.

## Toolchain

Rust 1.99.0 is pinned in `rust-toolchain.toml`. `rust-version` is 1.98,
because the Nix package builds with nixpkgs' rustc. Add dependencies with
`cargo add`.

## Release

See README "Develop and release": `just release-bump`, edit CHANGELOG,
commit and push, wait for the Nix Cache workflow, then `just release-verify`
and `just release-tag`.
