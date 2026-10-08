# Changelog

All notable changes to `hail` are documented in this file.

## Unreleased

- **An `fyi` never interrupts.** It is no longer typed into the recipient's
  pane; it arrives with their next prompt. Where the recipient's hooks have
  never run (no `hooked` mark from `deliver` in the last 7 days), it is typed
  as before, so it cannot strand. A quiet `fyi` exits 0 and leaves no pending
  record. Every other kind still types (quiet mail §1a).
- **`hail note <bead> '<headline>'` puts progress on the bead.** A bd comment
  signed with your seat (body on stdin); no mailbox, no pane, no receipt. A
  bd failure is an error that quotes bd (quiet mail §1b).
- **Holds lapse.** A `hold` lapses after 8h and a `block` after 7d, unless
  `--for 30m|8h|3d` (at most 7d) says otherwise; the envelope shows `until:`.
  Tools now serialize landing, installs and timing runs, so a hold is a
  person's decision, not a lock. A lapsed hold leaves every brief at once;
  its issuer's brief says so once, `release` still works on it, and `gc`
  deletes old and orphaned ones. The 33 holds recorded before 0.5 lapse 8h
  (7d for blocks) after they were sent (quiet mail §3).
- **Sub-agents have an address: `<seat>/<name>`.** Mail to
  `murail-2b/scout` goes to murail-2b's mailbox with `for: scout`, the
  envelope shows `for:scout`, and the parent relays it. `--as <name>` signs a
  sub-agent's send as `<seat>/<name>`, so replies come back the same way. The
  skill says a sub-agent runs no verb that reads mail (`inbox`, `deliver`,
  `brief`), since those would take its parent's mail (hail-b28).
- **A `from:` value works as a target.** `murail-1a/%5` is the pane `%5`,
  checked to be in seat `murail-1a`.
- **A seat-like name that isn't a seat is explained.** `murail-2b-scout`, when
  `murail-2b` is a seat, suggests `murail-2b/scout`.

## v0.4.1 - 2026-10-07

Two data fixes; nothing else changes.

- **Fixed: a headline-only message that was never typed was lost.** The
  prompt hook assumed every message without a body had been typed into the
  pane, so mail to a seat with no agent pane (or one held at a dialog, or not
  confirmed) was marked `injected` and never shown. The hook now injects
  every headline-only message as its envelope line. Lost since 0.4.0
  (hail-2en).
- **Fixed: a target of `..` wrote mail outside the store.** `hail .. fyi x`
  wrote `state/hail/new/<id>.md`, and `../..` wrote above it. An address part
  that is empty, `.` or `..`, or holds `/` or NUL, is now refused (exit 1),
  and so is `HAIL_SEAT=..` (exit 3) (hail-cr9).

## v0.4.0 - 2026-10-07

hail is now one Rust binary. The envelope, kinds and receipts an agent sees
are unchanged; identity, storage and setup are new. Spec:
`docs/2026-10-06-rust-port-spec.md`.

- **Identity is the seat.** A seat is the jj workspace or git root a process
  runs in, by name, or a `.hail-seat` file. It comes from the working
  directory, never from the process tree or `TMUX_PANE`, so a Codex command
  under the shared app-server can no longer sign as another pane (every Codex
  seat signed as `%2` in 0.3.6). Labels, `hail name`, incarnations and exit
  3 "label moved" are gone. `name`, `hello`, `who`, `resolve` and `id` stay
  as shims through 0.4.
- **Shared directories.** Several agents in one directory are told apart as
  `<seat>@<pane>`, accepted only for a Claude pane whose own directory is the
  seat. A send to the bare shared seat is refused (exit 3) with the
  sub-seats listed, so one agent can no longer drain another's mail.
- **Maildir store.** Each seat's inbox is `seats/<seat>/{tmp,new,cur}`.
  - A body is claimed by a rename, so exactly one reader wins and it is never
    injected twice (murail-m65jq).
  - A hook whose output never lands gives the mail back.
  - An id index (`ids/`) makes `sent`, `show` and `await` a few stats.
  - `await` polls every 100 ms and no longer needs fswatch.
- **Send takes the body on stdin:** `hail <seat> <kind> <<'EOF'`, with the
  headline on the first line. Backticks, `$()` and quotes arrive
  byte-identical.
  - A headline argument means stdin is never read, so a tool runner's open
    pipe cannot hang a send.
  - No `hail read` is needed before a send.
  - The 0.3 form (`--kind`, `--body`) still works.
- **Exit 5:** the message is in the inbox but was not typed (no agent pane,
  or typing not confirmed). `id=` is still printed, and stderr says not to
  resend.
- **reply:** names the sender's seat rather than its pane.
- **Delivery is capped per prompt:** at most five bodies (about 8 KB); the
  rest wait for the next prompt or `hail inbox`. `deliver` and
  `brief --hook` never fail a session: errors go to `hook-errors.log`, and
  any claim made before the error is given back.
- **The message file** cites `bead: <id>`; the comment number goes only to
  the sender's stdout.
- **Bounded brief:** five entries per section, `--all` for the rest. Holds
  sent to or by you show in full, and other seats' holds as a count. Pending
  sends expire from the brief after seven days. Nothing else expires.
- **`hail setup`** installs the Claude Code and Codex hooks. It shows a diff
  and asks first, replaces 0.3 hook lines in place, and keeps comments and
  other tools' hooks. `--check` reports drift.
- **`hail doctor`** checks the seat, tmux, the shared seats, orphaned
  sub-seats, the hooks and the state size; each problem names its fix.
- **New verbs:** `hail whoami` and `hail seats`. `hail gc` archives read mail
  older than 90 days.
- **Upgrading:** run `hail migrate` once after the upgrade.
  - It imports the 0.3 state; mail keyed by a live pane id goes to that
    pane's seat.
  - Until it runs, hooks are silent and other verbs ask for it.
  - `hail migrate --revert` goes back.
- **Speed:** hooks and receipt checks start no other process. They take about
  2 ms idle, against 15–175 ms for the bash version. A send's own work is
  under 20 ms, plus the 300 ms wait before Enter.
- **Tooling:**
  - `just check` adds fmt, clippy, cargo tests and `scripts/bench.sh`.
  - The scenario harness covers 48 scenarios, including shared seats, heredoc
    bodies, setup and migration.
  - CI runs on Linux and macOS, on the toolchain pinned in
    `rust-toolchain.toml` (Rust 1.99.0), under the nx-rs lint set (clippy
    pedantic, unwrap and expect denied).
  - The Nix package builds with `buildRustPackage` and is published to the
    `flowerornament` Cachix cache for four systems on every push to `main`.
    `just release-tag` refuses to tag until every build is cached.
  - Releases attach binaries for aarch64/x86_64 macOS and Linux.

From the unreleased 0.3.9, which ships here:

- The dialog guard no longer refuses an idle Codex pane. It matched the bare
  word `Approve`, and Codex prints `Approved` in its history and status line,
  so a pane at its prompt read as a permission dialog (exit 4) and agents
  learned to `--force` past it (herald-b6mzr). The guard now matches the
  dialogs' own question and option lines: Claude Code's as before, plus
  Codex's `Would you like to run the following command?`, `Yes, proceed`,
  `Yes, just this once` and `No, and tell Codex what to do`. New scenario 40.
- The skill says to read the pane before `--force`.
- The skill no longer says every over-cap headline is refused: only control
  kinds are; the rest fold into the body (since 0.2.4).

## v0.3.8 - 2026-10-06

- A send, its Enter and `hail keys` leave copy mode first. A pane scrolled
  with the mouse wheel is in copy mode and reads typed keys as mode commands:
  with vi keys the `:` of `kind:` opened a goto-line prompt that waited for
  Enter, and the envelope never reached the composer. New scenario 39.
- The skill says to run hail directly, never through `tmux run-shell` with a
  hand-set `TMUX_PANE`. Codex seats did that to dodge the `%2` identity bug
  fixed in 0.3.7, and every failure opened tmux's view mode in the pane the
  user was looking at.

## v0.3.7 - 2026-10-06

- A command run under Codex's shared app-server no longer takes the pane that
  started the daemon as its own. The daemon is a child of the first Codex
  seat's TUI, so the ancestor walk found that pane for every Codex seat: all
  of them signed, registered and read as `%2`. The walk now stops at
  `codex app-server`, and the pane is taken from the working directory, as
  for a daemon reparented to pid 1. New scenario 38.

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
