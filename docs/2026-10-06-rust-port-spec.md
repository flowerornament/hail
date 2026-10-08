# hail 0.4: the Rust port (spec)

Status: reviewed 2026-10-07 (%28: approve with three blockers, all taken; see §16), ready to build. It replaces `bin/hail` (bash, 1521 lines at 0.3.9) with one Rust binary. It applies slices S1 (seat identity) and S2 (Maildir claim) of `2026-09-30-simplification-study.md`, and keeps everything an agent sees today working. This spec draws on the study, its 2026-10-06 revision, and a census of `~/.local/state/hail` on 2026-10-06.

## 1. What 0.4 is, and what it is not

**It is:**
- the same tool, with the same verbs, envelope and kinds, in a fast, tested and readable codebase;
- identity derived from the workspace directory, never from a process tree or `TMUX_PANE`;
- a Maildir store with an atomic claim, an id index and bounded state;
- first-class setup: `hail setup` installs and checks the hooks and the skill, and `hail doctor` explains every failure.

**It is not:**
- *Waking agents without typing (S3).* The envelope is still typed into the pane. The transport sits behind one module, so S3 can replace it in 0.5 once Codex's `PostToolUse`/`Stop` hooks are proven.
- *Moving holds and obligations to bd (S4).* That needs `just land` changes in other repos. 0.4 keeps both in hail, bounded (§6.6).

## 2. A year from now

These are the conditions the design has to survive:

1. **More seats, more harnesses.** Today there are about 30 seats and 2 harnesses. A year from now, expect 50 or more and 3 or more, and harness internals keep changing. 0.3.7 broke because the Codex app-server moved in the process tree. *So identity depends only on the directory the agent works in, which hail controls through the workspace layout.*
2. **Volume.** Today about 6,700 messages and 1,500 obligations a week, with 144 MB of state after a month. That is about 350,000 messages a year. *So every hot path is O(1) or O(new), never O(history): an id index, no scans over read mail, bounded briefs, and a `gc` verb.*
3. **The protocol outlives the transport.** Typing into tmux panes is the weakest part. Hooks are getting richer, and Claude Code already exposes a messaging socket. *So the transport is a seam: envelope, store and receipts do not know about tmux.*
4. **Agents are the users.** They read the skill once per session, copy examples, and route around anything that fails. Examples are the 305 `tmux run-shell` workarounds and the `--force` habit. *So there are few verbs with few options, examples that work verbatim, and errors that name the fix. A guard that misfires is worse than no guard.*
5. **Bash is retired.** The tool must stay maintainable by a different agent each week. *So it has small modules with pure cores, unit tests beside the code, and one dependency per real need.*

## 3. Decisions at a glance

| # | decision | why |
|---|---|---|
| D1 | A **seat** is the basename of the jj workspace root, else the git root, else the directory holding a `.hail-seat` file (whose content names the seat). It comes from the process's working directory only. | It is the only fact that survives the Codex daemon. It equals today's labels, which seat.sh already derives the same way. |
| D2 | A directory holding more than one agent pane is a **shared seat**. Each Claude pane in it gets a checked sub-seat `<seat>@<pane>`. Sends to the bare shared seat are refused, with the sub-seats listed (exit 3). | Today three directories hold several agents (`peat`, `hail`, `.nix-config`). A shared mailbox would be drained by whichever agent prompts first: misdelivery with a receipt that says "injected". |
| D3 | Store: `seats/<seat>/{tmp,new,cur}`. The claim is `rename(new → cur)`. The receipt is the claimed file: its name says how it was claimed, and its mtime says when. | One claimer wins, so there is no double injection (m65jq). The receipt cannot exist before the claim. |
| D4 | `ids/<id>` is a symlink to the seat name, created exclusively. | Lookups take O(1) stats. Id uniqueness holds under any concurrency, and ids may keep their short format. |
| D5 | Send takes the body from stdin; the headline is the first line. The legacy `--kind`/`--body` form is still accepted. | It removes the sender-shell class: backticks, `$()` and apostrophes. It is one action per send. |
| D6 | Send needs no prior `hail read`. `type` and `keys` still do. | Sending to a seat is not driving a pane. The guard cost one call and one pane read per message, which DESIGN.md set out to remove. |
| D7 | The transport keeps today's behaviour: leave copy mode, refuse a dialog by its structure, type, verify, Enter. | This keeps scope to the language and the store. S3 replaces it later. |
| D8 | Hot paths spawn no subprocess. `deliver`, `brief`, `sent`, `show`, `await`, `whoami` and `inbox` touch only the filesystem. | Speed (§9), and hooks must not depend on tmux. |
| D9 | The transition (state migration, rollback, legacy shims) lives outside the core modules (§11). | The core reads as if the bash never existed. Retiring the transition is deleting one module. |

## 4. The agent interface

### 4.1 Verbs

```
hail <seat> <kind> [--re ID] [--scope S] [--for SPAN] [--as NAME] [--no-submit] [--force]   # body on stdin
hail sent <id>                    # delivered | injected <t> | read <t> | inline <t> | unknown
hail await <id>... [--timeout S] [--any]
hail inbox [--peek] [--all]       # unread bodies; claims them (read)
hail show <id>                    # one message, any seat, no claim
hail brief [--all]                # standing state, at most 15 lines unless --all
hail deliver [--format claude|codex] [--event E]   # for hooks
hail whoami                       # seat and how it was derived (filesystem only)
hail seats                        # every seat: wake pane, agent, unread, last activity
hail read <pane|seat> [N]  ·  hail type <pane|seat> <text>  ·  hail keys <pane|seat> <key>...
hail setup [--check] [--yes]  ·  hail doctor  ·  hail migrate [--revert]  ·  hail gc [--days N]
hail help [topic]  ·  hail --version
# hook-only: hail deliver --format <h>   ·   hail brief --hook (silent on error)
# test/automation: --no-wake on send writes the message and skips the transport
```

### 4.2 Sending

The canonical form:

```bash
hail murail-1b ask <<'EOF'
Review src/auth.ts before the merge; reply done with your verdict
The refresh path is in auth/refresh.rs:40-120. Coverage report: /tmp/cov.txt
EOF
```

**Headline and body:**
- With a positional headline (`hail murail-1b fyi 'gate green'`), stdin is never read and there is no body. An agent's tool runner often leaves stdin as an open pipe that never reaches EOF, so reading it could hang a send forever.
- Without one, stdin is read only when data arrives within 50 ms (the hooks' test, §8), then to EOF. The first line is the headline and the rest is the body. One blank line between them is customary and stripped. With no data: exit 1, `no headline: pass it as an argument or on stdin (heredoc)`.
- Control characters (C0 and C1, ESC especially) are stripped from the headline before it is typed. The body file keeps every byte.
- A headline over `HAIL_ENVELOPE_MAX` (400) is folded at a sentence boundary into the body, as in 0.3. A control kind over the cap is refused (exit 2).
- `--scope` stays a one-line flag.

**Legacy form, accepted for all of 0.4:** `hail <target> '<headline>' --kind k [--body X]` and the `send|message|msg` prefixes. It is recognised by `--kind` being present. `--body X` keeps 0.3's meaning exactly: `-` is stdin, an existing readable file is read, anything else is literal text. It prints no deprecation noise, because agents learn from the skill and not from warnings.

**Targets:**
- A seat name, which is the normal case, or a sub-seat (`hail@%28`).
- A pane id or tmux target (`%7`, `sess:1.2`), which resolves to the seat of that pane's directory, or to the pane's sub-seat when the directory is shared. Replies to 0.3 envelopes (`reply:%N`) keep working this way.
- A bare shared seat is refused (exit 3): `seat hail has 2 agents: hail@%26 hail@%28; address one`.
- `seat/%N`, the form `from:` prints: the pane `%N`, resolved as above and checked to be in that seat (exit 3 otherwise), so a `from:` value can be pasted as a target.
- `seat/name` (also `seat@%N/name`): a sub-agent. The message goes to the parent's mailbox with a `for: name` header and `for:name` in the envelope, and the parent relays it. `--as <name>` signs a send as `<mailbox>/<name>` in `from:` and `reply:`. Sub-agents have no mailbox of their own (quiet-mail design §2).
- The target is parsed at one boundary (`seat::Address`): `/` is split first, then `@`, and every part must name exactly one directory entry. An empty part, `.`, `..`, `/` or NUL is refused (exit 1).
- An unknown seat is an error that lists the near names (exit 1). One whose prefix is a known seat plus `-`, `_` or `.` also suggests the sub-agent address: `send to murail-2b/recip-consumer (its parent relays)`.

**Quiet `fyi` (0.5, docs/2026-10-08-quiet-mail-design.md §1):** an `fyi` is not typed when the recipient mailbox's `hooked` file (touched by every `deliver`) is under 7 days old. It arrives with the recipient's next prompt, exits 0, prints `quiet: arrives with <seat>'s next prompt` on stderr, and writes no pending record. Every other kind types as before, and so does an `fyi` to a mailbox whose hooks have not run.

**Output:**
- `id=<id>` on stdout, on exit 0 and on exit 5.
- On exit 5, stderr says `delivered to <seat>'s inbox; not typed (<reason>); do not resend; it arrives on their next prompt`. *(Amended 0.5.1, hail-2xl:)* when the mailbox has no `hooked` mark from the last 7 days, it says instead that no hook reads it, so it waits until someone runs `hail inbox` in the seat's root, and points to `hail seats`.
- *(0.5.1, hail-xe9)* When the sending pane is a Claude pane whose own directory is another seat, stderr warns that replies to the sending seat will not reach that pane. A warning only: the working directory still decides who sends.
- Warnings go to stderr, one line each, and say what to do.

### 4.3 The envelope (unchanged in form)

```
[hail kind:<k> from:<seat>/<pane> reply:<seat> id:<id> [for:..] [re:..] [scope:..] [until:..]] <headline>[ — hail inbox]
```

The only change: `reply:` names the seat, not a pane id, because seats are stable. `from:` keeps the `/<pane>` suffix for humans reading a pane, and the pane is omitted when unknown. The skill's rule "reply to the `reply:` value" keeps working.

### 4.4 Exit codes

| code | meaning |
|---|---|
| 0 | ok |
| 1 | usage or state error; the message names the fix |
| 2 | control-kind headline over the cap |
| 3 | seat problem: no seat here, a shared seat addressed bare, or a seat name already bound to another root (S1); replaces "label moved" |
| 4 | the target shows a permission dialog |
| 5 | delivered to the inbox but not typed (no agent pane, the pane is gone, or typing was not confirmed); the message is safe, so do not resend |

Exit 5 is new. Today these cases die with exit 1 after the message is written, which makes agents resend.

### 4.5 Retired verbs (shims for 0.4, removed in 0.5)

- `name <target> <label>`: exits 0 and prints `hail: labels are gone; <pane> is seat <seat> (from its directory)`. seat.sh calls it, and it must not fail a hook.
- `hello`: prints the seat.
- `who [seat]`: the `seats` row for that seat.
- `resolve <seat>`: prints the wake pane.
- `id`: prints the pane id when known, else exits 1.

## 5. Identity

```
seat_of(dir):  walk up from dir, stopping before $HOME (never a seat itself);
               the first ancestor with .hail-seat            → its trimmed content (must be [A-Za-z0-9._-]+)
               else the first ancestor with .jj/ or .git     → its basename (.git may be a file: worktrees, submodules)
               else none (exit 3 for verbs that need a seat)
               a name equal to a verb (send, sent, inbox, seats, …) is refused; doctor says to add .hail-seat
```

**Name collisions.** A basename is not unique (`~/code/foo` and `~/work/foo`). The first use of a seat writes `seats/<seat>/root`, holding the absolute root path. A different root claiming the same name exits 3 with `seat foo is bound to <root>; add .hail-seat in <other root>`. The check costs one read.

**Shared seats (D2).** A seat with more than one agent pane is shared, as `send` and `seats` see it when they list panes.
- **Sub-seats:** a Claude agent in a shared seat is `<seat>@<pane>`. Its pane is `TMUX_PANE`, accepted for signing only when that pane's `pane_current_path` maps to the same `seat_of`, it runs an agent that can hold a sub-seat, and the process is not Codex. That check rejects a stale or inherited `TMUX_PANE`, the bug class being removed.
- **Hooks:** `deliver`, `inbox` and `brief` read the sub-seat for `$TMUX_PANE` whenever its directory exists (a validated send created it) and the process is not Codex, and they read the seat too. They make no tmux call, so hooks stay subprocess-free. *(Amended 2026-10-07: an earlier `shared` marker routed this and stranded mail when sharing ended; it is gone.)*
- **Codex:** Codex panes never get a sub-seat, because their `TMUX_PANE` is the daemon's. A shared directory holding a Codex pane exits 3 for sends to it, with the fix: put that agent in its own jj workspace.
- **Pane ids are not stable** across a tmux server restart. A restart orphans a sub-seat mailbox, and `doctor` lists the orphans with their unread counts. This is accepted for an edge case; the long-term answer is one workspace per agent.

**Which directory each command uses:**
- **self:** `seat_of(cwd)`. `HAIL_SEAT` is honoured only when it equals `seat_of(cwd)` or `cwd` has no seat. It cannot override a seat the directory states, so a stale inherited variable is harmless.
- **target pane for a seat:** from one `tmux list-panes -a -F '#{pane_id}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_in_mode}\t#{pane_pid}'`, map each pane's path through `seat_of` (memoised per path), keep the panes in the seat, and prefer the one whose command is an agent (`claude`, `codex`, `node`; configurable later). One agent pane is the wake target. Zero means no wake, exit 5. Two or more makes a shared seat (above).
- **sender's own pane, for `from:`:** the sub-seat's pane, or the pane in the sender's seat when there is exactly one agent pane there, else omitted. No process tree is consulted anywhere.
- **Where Codex runs commands and hooks (verified 2026-10-06):** a Codex exec runs in the session's workspace (lsof: pid 37800 under the app-server, cwd `murail-1b`). Codex's SessionStart hook derived the right name from `jj workspace root` in seat.sh, so hooks run there too. `doctor` re-checks this per seat.

**Accepted cost:** an agent that runs `cd ../other-seat && hail …` signs as the other seat. The skill says to run hail from your workspace. `whoami` shows the seat, so an agent can check its identity instead of inventing a workaround.

## 6. Store

The root is `$XDG_STATE_HOME/hail` (default `~/.local/state/hail`). 0.4 writes only the new tree. Migration is described in §11.

```
seats/<seat>/tmp/<id>.md               being written
seats/<seat>/new/<id>.md               unread
seats/<seat>/cur/<id>.<how>.md         claimed; how ∈ injected|read|inline; mtime = receipt time
seats/<seat>/pending/<id>              my sends with no receipt yet (bounded, §6.5)
seats/<seat>/owed/<id>                 open obligations on this seat (ruling, go, ask)
holds/<id>                             holds and blocks in effect (global)
ids/<id> -> <seat>                     symlink, created exclusively; the id index
archive/<yyyy-mm>/<seat>/...           gc'd messages
```

The message file format is unchanged: header lines (`from`, `reply`, `kind`, `id`, `time`, `bead`, `see`, `re`, `scope`, `ask`), a blank line, then the body. Agents and humans `cat` these files.

### 6.1 Ids

- The format is `MMDDTHHMMSS-xxxx` (UTC, 16 random bits from `getrandom`). It is unchanged, because agents copy and type these.
- Uniqueness comes from `symlink(seat, ids/<id>)`, which fails if the entry exists. On EEXIST, draw again.
- The missing year is harmless: a reused id in a later year collides in `ids/` and is redrawn.

### 6.2 Send (store side)

1. Reserve the id.
2. Write `tmp/<id>.md` and fsync it.
3. Rename it to `new/<id>.md`. For a control kind, rename to `cur/<id>.inline.md` instead.
4. Record the state change: `owed/`, `holds/` or `pending/`.
5. Then the transport.

The message is durable before anything is typed. A failed wake leaves a deliverable message and exit 5.

### 6.3 Claim

`deliver` and `inbox` list `new/` (only unread mail, so this stays small).

1. For each entry, `rename(new/<id>.md → cur/<id>.<how>.md)`.
2. ENOENT means another claimer won: skip it.
3. After the rename, set the mtime to now.
4. Emit. A message whose body is just its headline is emitted as its one-line envelope, typed or not: the composer can lose typed text (a dialog, a cleared prompt), and a repeated line costs less than a lost message. *(Amended 2026-10-08, hail-2en: the hook used to emit nothing for these, assuming the envelope had been typed, so untyped ones were marked injected and never shown. A typed-marker design was rejected in review: the marker proves Enter was pressed, not that the text arrived.)*
5. If emitting fails (stdout closed, the hook was killed before the write finished), rename it back to `new/`. Delivery is at least once, under one id. The claim is never doubled.

A crash between the rename and the mtime update leaves the send time as the receipt time. That is harmless, because it is earlier, never later.

`inbox --peek` and `show` read without renaming.

### 6.4 Receipt lookup

`sent`, `await` and `show`:

1. `readlink ids/<id>` gives the seat.
2. Then stat `cur/<id>.injected.md`, `cur/<id>.read.md`, `cur/<id>.inline.md` and `new/<id>.md`.

That is at most 5 syscalls, and there is no directory scan. `await` polls this every 100 ms, so no fswatch is needed and the 1 s / 5 s latency is gone.

### 6.5 Pending (my sends without receipt)

- `pending/<id>` is written at send, except for a quiet `fyi` (§4.2).
- `brief` checks each pending id's receipt (§6.4) and deletes the entry once a receipt exists, or once it is older than 7 days, when it is reported once as expired.
- The directory holds only the open set. Today's `sent/` grows forever.

### 6.6 Obligations and holds, bounded

The semantics are unchanged: `ruling`, `go` and `ask` create `owed/<id>`; `done --re` removes it; `hold` and `block` create `holds/<id>`; `release --re` removes it.

New bounds, in `brief` only. Obligations never expire: hiding a live ruling by age would change what the protocol means.

*(Amended for 0.5, docs/2026-10-08-quiet-mail-design.md §3.)* Holds and blocks do lapse, because tools now serialize what holds were used for (landing, installs, timing). `hold` and `block` take `--for <span>` (default 8h for a hold, 7d for a block, at most 7d); the record carries `expires:` and the envelope `until:`. A record without `expires:` lapses at `time:` plus its kind's default. Lapse is a read-time filter: every reader agrees and none deletes. The issuer's own `brief` moves its lapsed holds to `holds/lapsed/` and says so once; `release` finds a hold there and says it had lapsed. `gc` deletes holds that lapsed more than `--days` ago, and lapsed ones whose issuer has no mailbox.
- `brief` shows at most 5 obligations and 5 holds, newest first, then `… N more, oldest Nd (hail brief --all)`.
- The 376 obligations on `%1` are an artefact of pane keys. Migration moves them out of every live seat (§11.1).

Closing stays explicit (`done --re`). A reply of another kind with `--re` does not close an obligation, because progress `fyi`s carry `--re` too.

### 6.7 gc

`hail gc [--days N]` (default 90) moves `cur/` messages older than N days into `archive/<yyyy-mm>/<seat>/`, removes their `ids/` entries, and appends `<id> <yyyy-mm> <seat>` to `archive/index` (one line per message, append-only). `sent` and `show` consult that index only when `ids/` misses. It never runs implicitly in 0.4. `doctor` reports when gc is due (`cur/` over 5,000 files).

## 7. Transport (tmux)

**Socket detection is unchanged:**
1. `HAIL_SOCKET`, or `TMUX_BRIDGE_SOCKET`;
2. a live `$TMUX`;
3. a scan of `/tmp/tmux-$UID`, `/private/tmp/tmux-$UID` for the server that owns the target;
4. the default server.

**Send transport, in order:**
1. `list-panes` (one call) resolves the wake pane and its mode.
2. **Dialog guard:** `capture-pane`, last 8 non-blank lines, matched against the structural patterns of 0.3.9. These are a const table in `transport/dialog.rs`, with each harness's strings and the version they were taken from. `--force` skips it.
3. **Leave copy mode and type, in one tmux call:** `send-keys -X cancel ; send-keys -l -- <envelope>`, with the cancel only if the pane is in a mode.
4. **Verify:** poll `capture-pane` every 25 ms for up to 10 s for the envelope's `id:<id>` token, whitespace removed so a wrapped line still matches. Seeing it proves the agent has read the text, so Enter is read in a later batch; Codex treats an Enter read together with typed text as a pasted newline and leaves the envelope in the composer. The probe must be unique to this send: the opening characters repeat in every envelope from one seat, and an earlier one on screen matched before the new text was read (hail-lha). It never retypes.
5. **Submit:** wait, then `send-keys Enter`. The wait exists for paste-burst detection after the text has rendered, so a faster verify does not shorten it. It is 300 ms in 0.4, one named constant per harness in `transport/` (`SUBMIT_DELAY`).

   **Trial before lowering:** 100, 150 and 200 ms, in Claude and Codex panes, idle and loaded, 200 sends each, counting envelopes left unsubmitted. Lower it only at zero misses.
6. *(Removed in 0.5.1: hail no longer posts to bd; see §17's amendment.)*

**The seam:** `trait Wake { fn wake(&self, pane, envelope) -> Result<Woken, WakeError> }`, with one implementation, `TypedEnvelope`. S3 adds `TypedToken` and later a harness-native channel. Nothing outside `transport/` calls tmux except `seats`, `read`, `type`, `keys` and `doctor`, through the same `Tmux` client.

## 8. Hooks and integration

| event | Claude Code | Codex | command | output |
|---|---|---|---|---|
| session start | `SessionStart` | `SessionStart` | `hail brief --hook` | plain text, or nothing |
| prompt submit | `UserPromptSubmit` | `UserPromptSubmit` | `hail deliver --format <h>` | `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":…}}`, or nothing |

**Behaviour shared by every hook command:**
- Each one reads and ignores stdin JSON without blocking: it reads to EOF only when stdin is a pipe, with a 50 ms cap.
- **PostToolUse delivery waits for S3 (0.5).** While the envelope is still typed, a second delivery path would claim a body mid-turn and then deliver its envelope late.
- `deliver` touches `seats/<mailbox>/hooked` for each mailbox it reads, every run: the evidence a send uses to make an `fyi` quiet (§4.2).
- When the directory is not a seat, it is silent and exits 0. No `[ -n "$TMUX_PANE" ]` wrapper is needed, since identity no longer depends on tmux, so the hook lines become `hail deliver --format claude`.
- Exit 0 always. A hook never blocks a session, and errors go to `~/.local/state/hail/hook-errors.log`, capped at 1 MB.

**`hail setup`:** the one installer.
- It edits `~/.claude/settings.json` (serde_json, preserving unknown keys and order) and `~/.codex/config.toml` (`toml_edit`, preserving comments and layout).
- It replaces any older hail hook lines, including the `[ -n "$TMUX_PANE" ]` form, in place. It never adds a duplicate.
- It prints a diff and asks for confirmation unless `--yes` is given. `--check` reports drift and exits 1 when there is any.
- It reminds you that Codex needs `/hooks` trust once.
- The skill link stays Home Manager's job (`programs.hail.skill`). `setup --check` only verifies the link.
- `hooks/README.md` keeps the manual blocks for people who do not use `setup`.

**`hail doctor`** checks, one line each with the fix:
- the binary version, and the skill's version marker matching it;
- tmux reachable, with the socket source;
- this directory's seat;
- the wake pane for this seat (zero, one, or several agent panes);
- the hooks installed in each harness, and their current form;
- the state root writable, with sizes;
- whether gc is due;
- legacy state present but not migrated;
- for each seat: its root binding, a shared marker with live panes, orphaned sub-seats, and a verb-named seat.

## 9. Speed

Budgets are measured with `hyperfine --warmup 5` on the dev machine, idle, and enforced in CI at three times the budget:

| path | 0.3 (bash) | 0.4 budget |
|---|---|---|
| `deliver`, empty inbox | about 15 ms idle; 70–83 ms at load 170 | **< 2 ms** |
| `brief`, typical | about 40 ms | **< 3 ms** |
| `sent`, `show`, `whoami` | 10–20 ms | **< 2 ms** (`whoami` reports the seat only; `seats` shows panes) |
| `send`, wall clock to Enter | about 0.6–2.5 s | **≤ 400 ms**, dominated by the 300 ms submit wait; the tool's own work is < 20 ms |

How these are reached:
- no subprocess on any hot path (D8);
- one `list-panes` per send;
- the cancel and the typing chained into one tmux call;
- 25 ms verify polling;
- `seat_of` from a few `stat`s;
- release profile: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.

## 10. Code

### 10.1 Layout (one crate, binary plus lib for tests)

```
src/
  main.rs              argv → Cli → run() → exit code; nothing else
  cli.rs               clap derive; legacy-form normalisation; help topics
  error.rs             enum Error { Usage, State, OverCap, NoSeat, Dialog, NotWoken } → exit code
  seat.rs              seat_of(dir), whoami, the seat list (pure + fs)
  store/
    mod.rs             Store { root } and paths
    ids.rs             reserve(), lookup()
    message.rs         Message { headers, body }; parse/render (pure, round-trip tested)
    mailbox.rs         deliver_new(), claim(), unclaim(), receipt()
    state.rs           owed, holds, pending; bounded listing
    gc.rs
  envelope.rs          render(), fold_headline()  (pure)
  transport/
    mod.rs             trait Wake; TypedEnvelope
    tmux.rs            Tmux { socket }: run(), list_panes(), capture(), send_keys()
    dialog.rs          const PATTERNS (pure, table-tested)
  hooks/
    output.rs          JSON shapes per harness and event (pure)
    setup.rs           settings.json / config.toml merge (pure over text, then fs)
  commands/            one file per verb: send, receive (deliver, inbox, show), receipts
                       (sent, await), brief, seats, panes (read, type, keys), setup, doctor, gc
  migrate.rs           0.3 → 0.4 state import (§11)
```

### 10.2 Rules

- **Pure cores, I/O at the edges.** `envelope`, `message`, `dialog`, `hooks::output`, `hooks::setup` (text in, text out) and `seat_of` (over a `Fs` trait) have no process or tmux access, and carry their unit tests in the same file.
- **Errors carry their fix.** Every `Error` variant's message ends with what to run. The exit code is a method on the enum, not scattered `exit()` calls.
- **No `unwrap()` outside tests,** enforced by clippy (`unwrap_used`, `expect_used` = deny in `src/`).
- **No `unsafe`** (`#![forbid(unsafe_code)]`). Nothing needs it now that there is no process-tree walk.
- **Time:** `jiff::Timestamp` everywhere. Formats live in one place (`message.rs`).
- **Comments say why,** in the style of the bash comments (incident ids where they explain a rule: m65jq, 4vc8v, b6mzr).

### 10.3 Crates

| crate | use | why this one |
|---|---|---|
| `clap` (derive) | the CLI | readable declarations, generated usage errors; startup cost is negligible next to a 2 ms budget |
| `serde`, `serde_json` | hook I/O, settings.json, `--json` later | standard |
| `toml_edit` | `~/.codex/config.toml` merge | keeps comments and layout; plain `toml` would rewrite the user's file |
| `jiff` | timestamps | correct, small API, by the regex author |
| `getrandom` | id randomness | no RNG state to seed |
| `similar` | the diff printed by `setup` | small, no deps |
| dev: `tempfile`, `assert_cmd`, `predicates`, `proptest`, `insta` | tests | snapshot help and brief output; property tests for the parser and folding |

Not used, on purpose:
- `tokio`: there is no concurrency to manage.
- `notify`: 100 ms polling of one path is simpler and portable.
- `regex`: dialog patterns are literals.
- `sysinfo`, `libc`: there is no process walk.
- `anyhow`: exit codes need typed errors.
- `chrono`.

## 11. Migration and rollout

### 11.1 Migration (`hail migrate`, explicit, once)

Migration is a step of its own, not a side effect. The first command after an upgrade is usually a hook, and importing about 11,000 files there risks the hook timeout and a killed import holding the lock.
- **Until it runs:** hooks are silent, and every other verb exits 1 with `run: hail migrate`.
- **The lock:** `state/.migrate.lock` holds a pid and a start time, and is taken over when that pid is gone.

**Messages:**
- `inbox/<key>/<id>.md` with no `.read` goes to `seats/<key>/new/`.
- With a `.read`, it goes to `cur/<id>.<how>.md`, with the mtime set to the receipt time.
- Every message gets an `ids/` entry.

**Keys:**
- A key that is a seat name stays.
- A pane-id key (`%14`) that is a live pane maps to `seat_of(pane_current_path)`, or to its sub-seat when that directory is shared. Unread mail lands where the agent will read it: today `%14` has 6 unread, `%17` 2, `%28` 1.
- A pane-id key whose pane is gone goes to `seats/legacy-%1/`, reachable by `show` and `sent` but never delivered.

**State:**
- `obligations/<key>/` goes to `seats/<key>/owed/`, mapped the same way as messages.
- `holds/` stays.
- `sent/` goes to `pending/`, with only entries under 7 days old and no receipt.
- `identity/`, `incarnation/` and `read/` are moved to `archive/0.3/`.

**Then:** the old tree is renamed to `archive/0.3/`. Migration is idempotent and logs counts to stderr once.

### 11.2 Rollout

1. **Build in the repo, beside `bin/hail`.** `test/run.sh` takes `HAIL_BIN`. The scenarios that survive (the mapping is in §12) pass against the Rust binary.
2. **Shadow week on this machine.** `hail-next` is installed beside `hail`. Agents keep using 0.3. A parity script replays a copy of the state through both binaries' read-only verbs (`sent`, `show`, `brief`) and diffs the output.
3. **Cutover.** Release 0.4.0, run `nx upgrade hail`, then `hail migrate`. The skill updates in the same release. Between the two commands, hooks stay silent and sends wait (§11.1).
4. **Rollback.** `hail migrate --revert` restores `archive/0.3/`. Mail sent under 0.4 since the cutover is copied into `inbox/<seat>/` in the 0.3 format.
5. **0.5.** Remove the shims and `migrate.rs`. Start S3.

## 12. Testing

**Unit tests** (in `src/`):
- the message parse/render round-trip (proptest, any bytes in the body);
- `fold_headline` (proptest: the result ≤ cap, a prefix of the input up to the ellipsis, sentence-preferring);
- the dialog table, the hook JSON (insta);
- the setup merges (fixtures: an empty file, an existing hail line in the old form, comments, unrelated hooks);
- `seat_of` over a fake fs.

**Integration tests** (`tests/`, no tmux), using `assert_cmd` with a temp `XDG_STATE_HOME`:
- send with `--no-wake`, then deliver, sent, await, brief and show;
- claim races: 64 threads calling `deliver` on one seat, so each message is claimed exactly once;
- 1000 parallel sends, so no id collides;
- a crash between claim and emit (simulated by a closed stdout), which redelivers under the same id;
- migration from a fixture 0.3 tree.

**Scenario harness** (`test/run.sh`, real tmux, `HAIL_BIN`):
- **Kept as is:** 1–10, 12, 13, 16–19, 21, 22, 24–27, 29, 31–36, 39, 40.
- **Rewritten for seats:** 11, 28, 37, 38. These become: a seat from the directory, an agent pane chosen over a shell pane, two agent panes giving exit 5, and a stale `TMUX_PANE` that is ignored.
- **Read-guard expectations removed from send:** 30.
- **Replaced by the cargo gate:** 15.
- **Budget lowered to 10 ms:** 23.
- **New:** a heredoc body with backticks, `$()` and `'` arrives byte-identical; `setup` in a temp HOME is idempotent; migration then `brief`.

**Perf gate:** `just bench` runs hyperfine on the hot paths, and CI fails at three times the budget.

**The gate** (`just check`): `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`, `bash test/run.sh`, `just bench`.

## 13. Packaging, release, docs

**Version:**
- `Cargo.toml` is the one version source.
- `release.py` reads it and checks that `hail --version`, the CHANGELOG and the Nix build agree.
- `package.nix` becomes `rustPlatform.buildRustPackage` (`cargoLock.lockFile`), and still installs the `tmux-bridge` symlink and `share/hail/skills/hail`.

**CI:**
- Linux and macOS: the cargo gate plus the scenarios.
- Nix build smoke.
- Release artifacts: `hail-<ver>-aarch64-apple-darwin` and `x86_64-unknown-linux-gnu` tarballs in addition to Nix.

**Docs:**
- `README.md`: install, setup, and a 10-line tour.
- `DESIGN.md`: updated for seats and the Maildir store.
- `docs/` keeps the studies.
- `skills/hail/SKILL.md` rewritten for 0.4. It is shorter, because the identity section shrinks to `whoami` and the send example is the heredoc. The skill carries `version: 0.4.x` in its front matter, so `doctor` can flag a stale link.
- `hail help <topic>` pages are generated from the same text the skill quotes, so there is one source.

## 14. Questions settled in review

| | question | answer |
|---|---|---|
| Q1 | Two agents in one directory | Checked sub-seats for Claude panes (D2, §5). A Codex pane in a shared directory needs its own workspace. |
| Q2 | Is `reply:<seat>` safe? | Yes. No scripts parse `reply:%`. Pane targets keep resolving for old envelopes. |
| Q3 | Expiry of obligations | None. Bounded in `brief` only (§6.6). Pending sends expire at 7 days. |
| Q4 | Lower the 300 ms submit wait? | Not in 0.4. A named per-harness constant, and the trial in §7. |
| Q5 | PostToolUse delivery in 0.4? | No. It ships with S3 in 0.5. |

## 15. What is new evidence in this spec

- **seat.sh has the same lookup bug.** murail's `scripts/seat.sh` resolves its pane with the same ancestor walk, so every Codex seat start renamed `%2` (herald-1b's pane). That is the "label on `%2` flips back to murail-2b" in herald-b6mzr. Under 0.4, seat.sh's `hail name` call becomes a no-op. Its `select-pane -T` (the pane title) still mislabels until seat.sh is fixed. That is a murail change: the fix is to drop the ancestor walk and use the single pane whose path is the workspace root.
- **Environment variables are not identity.** A Codex launched from a Claude pane inherits `CLAUDE_*` (seat.sh documents this), and Codex commands inherit the daemon's `TMUX_PANE`. Only the working directory is reliable for both harnesses.

## 16. Review record

Reviewed by the Claude session in `%28`, 2026-10-07 (message 1007T053740-630c). Its verdict: approve, with blockers on Q1, on stdin handling for send (B1) and on when migration runs (B2), plus unread pane-keyed mail (B3). It also raised six should-fixes: seat-name collisions, `seat_of` edge cases, where Codex runs hooks, exit 5 wording, control characters in headlines, and the legacy `--body` meaning. All of it is taken except one suggestion: closing an obligation on any `--re` reply (§6.6 says why).

## 17. Implementation notes (2026-10-07)

Where the code differs from the text above, and why:

- **Agent panes.** A pane is an agent when its foreground command is
  `claude` or `codex`, or when its root shell has one as a direct child. tmux
  reports a tool's process as the foreground command mid-turn, so the
  foreground check alone misses busy agents. The child check costs one `ps`,
  only on `send`, `seats` and `migrate`.
  - `HAIL_AGENT_COMMANDS` overrides the list. The scenario harness uses it to
    drive `bash` and `cat` panes as agents.
- **Shell panes are never woken.** A seat whose only panes are shells gets
  the message in its inbox, with exit 5. Typing an envelope into a shell
  runs it as a command.
- **Holds in the brief.** The brief shows holds sent to or by this seat in
  full; others are one count line. On the real state, all 45 holds were sent
  to one seat each, and every brief carried five unrelated ones.
- **Migration of pane-keyed state.** Unread mail keyed by a live pane id
  goes to that pane's seat (B3). Obligations and pending sends keyed by a pane
  id go to `legacy-%N`. On the real state these were 376 obligations on `%1`
  and 300 on `%2`, artefacts of the 0.3 identity bug that would otherwise have
  landed on herald-1a and herald-1b. A dry run on a copy of the real state
  migrated 11,329 messages in 6 s.
- **Closed stdout.** Rust reopens a closed fd 1 as `/dev/null` and reports
  every write as successful. `deliver` therefore claims nothing when stdout
  is closed or is `/dev/null`; a broken pipe still unclaims.
- **Two Claude config directories.** `setup` and `doctor` cover
  `~/.claude/settings.json` and `$CLAUDE_CONFIG_DIR/settings.json` when they
  differ. On this machine, sessions run with `CLAUDE_CONFIG_DIR=~/.claude-work`,
  whose settings had no hail hooks, so their bodies were never injected.
- **A seat is known** once it has a mailbox directory, or while a pane sits
  in it. A migrated seat with mail and no live pane still takes sends.

### Implementation review (2026-10-07, %28, message 1007T062241-1b86)

Four blockers, all fixed, each with a test:

1. **Sub-seat mail stranded when sharing ended.**
   - **Fix:** an agent's mailboxes are now its sub-seat, whenever one exists
     for `$TMUX_PANE`, plus its seat. `deliver`, `inbox`, `brief` and `done`
     read both, and the `shared` marker no longer routes anything; it only
     informs `doctor`.
   - **Codex:** a process carrying `CODEX_THREAD_ID` or `CODEX_SESSION_ID`
     never reads or signs as a sub-seat (review item 5). That is the cheap,
     specific check: Codex's commands carry those variables and the daemon's
     `TMUX_PANE`.
   - **Test:** scenario 46.
2. **A headline argument dropped a heredoc body.**
   - **Fix:** stdin is checked for 10 ms; a heredoc or file is readable at
     once and becomes the body. An idle open pipe is still never waited on
     (scenario 42).
   - Without a headline argument, the wait for stdin is now 2 s, so a slow
     producer gets through. Only the error path pays it.
   - **Test:** scenario 47.
3. **Migration stranded or flooded unread mail.**
   - **The cap:** `deliver` injects at most 5 bodies or 8 KB per prompt, then
     says how many wait.
   - **Pane-keyed mail:** it maps to the pane's seat only when it is unread
     and under two days old. Everything else keyed by a pane id is parked as
     `legacy-%N`.
   - **Reports:** `migrate` lists unread mail in seats no pane sits in, and
     `doctor` keeps reporting it.
   - **Test:** scenario 48.
4. **`deliver` was not hook-safe.**
   - `deliver` and `brief --hook` always exit 0, and log errors to
     `hook-errors.log` (rotated at 1 MB).
   - A failure mid-delivery gives back every claim it made.
   - **Test:** `tests/cli.rs::a_failing_claim_never_fails_the_hook`.

Also fixed:
- revert maps `<seat>@%N` and `legacy-%N` back to the 0.3 key `%N`;
- a `HAIL_SEAT` seat is not bound to a root;
- the migrate lock is published by hard link, with its content already
  written;
- gc writes the index line before each move;
- `out!`/`outln!` end quietly on a closed pipe;
- agent detection also checks the pane's root process by name. A pure parser
  for the `ps` table is unit-tested, and scenario 45 runs real detection with
  no stand-in agent list.

**Amended 2026-10-08 (0.5.1): hail no longer talks to bd.** Posting bodies to beads, bead-id detection, the `bead:` header and `hail note` are gone: the coupling tied hail to another moving target (bd's versions, its Dolt server, its timeouts) for little gain, since an agent posts to its tracker itself. `--bead` is accepted and ignored with a notice, and `hail note` says what to use, until 0.6. Old files keep their `bead:` header; nothing reads it.

**Format change, amending §6 (superseded by the amendment above):** the message file cites `bead: <id>`. The
comment number goes to the sender's stdout (`bead=<id> comment=<n>`), and the
`(comment n)` / `(not posted)` annotations and the `see:` line are gone. The
comment is posted after the wake, by which time the message may already be
claimed.

### Code-quality review (2026-10-07, %28, message 1007T074815-0fda)

The verdict was "yes, I would maintain it". Its items, all done before the release:
- **Typed domain:**
  - `seat::Addr` (a seat or a sub-seat) replaces the `@` string convention;
  - `store::ids::Id` replaces three separate id checks;
  - `transport::tmux::Agent` makes the sub-seat rule a method;
  - `store::records::{Entry, Pending}` keep the header names in one place;
  - holds have their own store API, with no empty-string seat.
- **One routing module** (`route.rs`): one parse feeds two policies, Mail for sends and Drive for `read`, `type` and `keys`. Its module doc says why they differ.
- **`Boxes { primary, seat }`** replaces indexing into a mailbox list.
- **`send::Form`** separates the 0.3 form, so 0.5 deletes one variant.
- **The time limits** live in `policy.rs`.
- **stdin** is capped at 30 s and 16 MB once data flows, so `yes | hail …` cannot hang a send.
- **No hidden writes:** `doctor` checks seat bindings without writing, and `seats` writes nothing. The dead `shared` marker, `lib.rs` and the unused dev-dependencies are gone.
- **insta snapshots** pin the help map, the send page, a brief and the hook JSON.
