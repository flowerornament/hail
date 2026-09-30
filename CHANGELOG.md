# Changelog

All notable changes to `hail` are documented in this file.

## Unreleased

## v0.3.6 - 2026-09-30

- A send no longer withholds Enter when the target's foreground command is a
  shell. tmux reports the process an agent's tool is running, so every agent
  mid-command looked like a shell and envelopes sat unsubmitted in composers.
  The rule, its stderr note and the `--force` bypass for it are gone.
- Typed-text verification polls for up to two seconds instead of looking once
  after 150 ms, and never clears or retypes. Under load the old path missed
  the redraw, sent eight Ctrl-U into the composer, retyped, and then left the
  envelope unsubmitted; that doubled messages and ate drafts. When the text
  still does not show, hail says so and names the `hail keys <target> Enter`
  that submits it.

- `hail help state` says how to lift stale holds silently: delete the hold
  file. No flag needed; the files are the state.

## v0.3.5 - 2026-09-30

- Removed the unsent-draft guard. In two days it refused sends for a subagent
  row, a queued-message placeholder, ghost-text suggestions and a grey glyph,
  grew a colour parser and three regexes, and never once caught a real draft.
  A send now types the envelope after whatever the composer holds, as it did
  before 0.2.5. Exit code 5 is gone; the permission-dialog guard (exit 4) and
  `--force` stay.
- Removed the "only the issuer can release" rule: a hold issued before its
  pane was labeled could not be released after `hail name`. Anyone may now
  release a hold; `hail brief` still shows who issued it.

## v0.3.2 - 2026-09-30

- The unsent-draft guard reads the composer in colour. Claude Code draws its
  prompt glyph and ghost-text suggestions in grey (or dim) and typed text in
  the default colour, so only default-colour text in the composer is a draft.
  A ghost-text suggestion (any text, not only the known placeholders) held a
  GO for 40 minutes. Panes that draw no rules keep the last-row rule.
- The rule-less fallback (Codex `›`, shells) reads its row in colour too, so a
  dim suggestion there is not a draft either.

## v0.3.1 - 2026-09-29

- The unsent-draft guard no longer mistakes Claude Code's background-agent
  panel for a draft. With many subagents running, the panel pushed the
  composer's rules out of the guard's window and the selected `❯ ◯ agent` row
  read as a draft, which trained senders to reach for `--force`. The guard now
  looks 60 rows up for the rules, ignores agent-status rows and the "Press up
  to edit queued messages" placeholder, and no longer exits silently when the
  last rows are all status rows.
- `hail show <id>` prints one message body by id from any inbox on this
  machine, with its receipt, and writes nothing. A send without `--kind`
  names it in the error.

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
