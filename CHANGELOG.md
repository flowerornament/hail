# Changelog

All notable changes to `hail` are documented in this file.

## Unreleased

## v0.3.0 - 2026-09-29

- `hail --version` and `hail -V` join `hail version`; `hail version --json`
  prints `{"name": "hail", "version": "..."}` for scripts.
- Help is a map plus detail pages: `hail --help` lists every command grouped
  by intent on one screen; `hail help <topic>` (send, kinds, receive,
  identity, panes, state) and `hail <command> --help` carry the options,
  envelope format, guards, state files, environment and exit codes. An
  unknown command or topic exits 1 and names `hail --help`.
- One version source: the `VERSION=` line in `bin/hail`. `package.nix`, the
  flake and the test harness read it from there, so the package can no longer
  drift from the script. The flake gains `apps.default` for `nix run`.
- Release tooling: `just release-bump`, `just release-verify` and
  `just release-tag` (`scripts/release.py`) bump the script and changelog,
  gate on the checks and a Nix build, then push an annotated `vX.Y.Z` tag and
  move the `release` branch to it. Pushing the tag publishes a GitHub release
  with the changelog section and the script as an asset.
- CI runs the shell lint, the release-script tests, the tmux scenario harness,
  a Nix build and a Home Manager module smoke test.

## v0.2.5 - 2026-09-29

- An empty prompt row wrapped into the agent panel is no longer mistaken for an
  unsent draft.

## v0.2.4 - 2026-09-21

- A headline over the cap is folded into the body at a sentence boundary
  instead of being refused; control kinds are still refused.
- The envelope cap is back to 400 characters, a useful length for most
  messages.

## v0.2.3 - 2026-09-05

- `--body` takes literal text as well as a file path or `-`.
- The over-cap refusal names `--body`; `brief` lines follow the cap.

## v0.2.2 - 2026-09-05

- A message with no `--body` is complete in the envelope: no fetch hint,
  nothing injected, receipt still written.
- Wording: the shell-pane note, from/reply, no double inbox, what a restart is.

## v0.2.1 - 2026-09-05

- `name` mints the incarnation; `hello` is idempotent; hooks drop `hello`.
- `read <target> N` returns exactly N lines.
- `brief` and `who` report a restarted pane.
- A send into a shell pane types the envelope but does not submit it.
- Control kinds are delivered once.
- The hooks README leads with the user-level install.

## v0.2.0 - 2026-09-05

- `deliver` for prompt hooks, `brief` for session hooks and humans, stateful
  kinds (obligations, holds), identity (labels, incarnations), the bare send
  form, auto-submit.
- Agent skill and hooks README.

## v0.1.0 - 2026-09-05

- First cut: envelope in the pane, body in the inbox, receipts, `await`.
