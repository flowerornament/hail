# hail simplification study (2026-09-30)

A whole-system study of hail and the jj-ops workflow, requested by Morgan after a long run of hail defect fixes. It was read-only: sources, docs, skills, tracker, memory notes, and a live state and process census. murail-1a banked it. Citations are to `bin/hail` at 0.3.3 (d20de6b).

## Revision (2026-10-06)

Six days and five releases (0.3.4–0.3.9) later, the evidence strengthens the design and changes its order of work. The original text below stands as written, except where it is marked.

### New evidence

| class | what happened | fix | learning |
|---|---|---|---|
| identity, a second miss | The Codex app-server is now a child of herald-1b's Codex TUI: pid 55572 → 13408 → 13375 → 3484, the root of `%2`. The ancestor walk therefore found `%2` for every Codex seat, and the 0.3.4 directory fallback never ran. In the 7 days to 10-07, 1165 messages were signed `*/%2`. At least 338 of them carry another seat's label: murail-1b 157, murail-2b 135, murail-4b 26, murail-4a 20. More of herald-2b's sends arrived as `herald-1b/%2` (herald-b6mzr). | 0.3.7: the walk stops at `codex app-server`. Scenario 38. | The daemon's place in the process tree depends on who started it: ppid 1 on 09-30, a pane's child on 10-06. Every process-tree rule is a guess about another program's internals. Two fixes in a row missed. |
| workaround spill (new) | Agents route around hail defects with raw tmux. murail-2b ran `tmux run-shell -t %8 -c ~/code/murail-2b 'env TMUX_PANE=%8 hail …'` 305 times from 10-05. herald-2b detached its invocations from the daemon. herald-2b also typed `send-keys -t %1 Enter` into another desk (herald-b6mzr). Each `run-shell` failure opened tmux view mode in the pane Morgan was looking at, and waited for Enter there. | 0.3.8: the skill says to run hail directly. | Agents find and keep any escape hatch they can reach, and it bypasses every guard. A correct default is the only real fix. Nothing should honour a hand-set `TMUX_PANE`. |
| delivery: copy mode | A pane scrolled with the mouse wheel is in copy mode. Typed envelope bytes ran as mode commands: with vi keys, the `:` of `kind:` opened a goto-line prompt. | 0.3.8: `leave_mode` before typing. Scenario 39. | Yet another UI state that typed content can hit. |
| delivery: dialog false positive | The dialog guard matched the bare word `Approve`. Codex prints `Approved` in its history and status, so idle Codex panes were refused with exit 4, and desks learned to `--force` (herald-b6mzr). Codex seats run with approvals bypassed, so they rarely show a real dialog. | 0.3.9: the guard matches each dialog's question and option lines. Scenario 40. | A false positive in a guard trains agents to force past it, and then the guard protects nothing. |
| sender text | In one six-day Codex session (murail-4a), run-together words grew from 0–1 per headline (10-01) to 5–29 (10-07), including headlines of 274 characters, far under the cap. The skill still said over-cap headlines are refused, which has been stale since 0.2.4. | 0.3.9: skill corrected. | Not a hail defect: the headline is model prose and degrades as a session ages. Restarting the session fixes it, not the tool. |
| state | 45 holds (40 at the study, newest 10-07). 2751 obligation files, 1478 written in the last 7 days. 11,193 inbox messages, 6,669 in the last 7 days, 268 without a receipt. 144 MB in total. Dead stores keyed by pane id, such as `inbox/%1` with 2306 files, are unchanged. | none | The growth rate makes S4 urgent rather than tidy. |
| latency | On a host at load average 170, an empty `deliver` with no tmux server took 70–83 ms against its 50 ms budget, at HEAD and before any change. That is bash startup and script parsing alone. | none | This bears on the implementation language (below). |
| Codex hooks | The 0.160.1 binary names `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStop`, `PreCompact`, `SessionEnd` and `additionalContext`. | none | The precondition the study set for S3 is probably met. Confirm it with a config test before relying on it. |

### Progress against the plan

- **Deleted since the study:**
  - the draft guard and its colour parser (0.3.5);
  - retyping and vim repair, and the shell-foreground rule (0.3.6).
- **Added since the study:** the `codex app-server` stop (0.3.7), `leave_mode` (0.3.8), and a longer dialog pattern (0.3.9).
- `bin/hail` has gone from 1571 lines to 1521.
- None of S1–S5 has started. Every fix since the study patched the model of the pane as identity and typing as transport, which the study recommends removing.

### What changes

1. **S1 comes first, and no more `own_pane` patches.** Seat-from-cwd would have prevented both identity misses and the `run-shell` workaround. Add these reds:
   - a daemon that is the child of another seat's pane (scenario 38 today);
   - no command honours a hand-set `TMUX_PANE`;
   - `hail whoami` prints the seat, so agents can check their identity without inventing workarounds.
2. **S3: the wake still types one token.** It must leave copy mode, and refuse only on a dialog's structure. Scenarios 39 and 40 carry over.
3. **S4 moves up, before S3.** State grows by about 1500 obligations and 6700 messages a week.
4. **Language: build v0.4 in Rust instead of shrinking the bash.** Porting the current 1521 lines would port the features this plan deletes.
   - Write the redesigned surface (S1 and S2 first) as a Rust binary.
   - Keep the scenarios that survive the redesign as the oracle.
   - Run v0.4 behind the planned one-week dual-read migration.
   - Rust gives startup in milliseconds for the hooks, atomic renames and process inspection without `ps | awk`, and unit tests for the parts that today can only be tested through a tmux server.
   - The risk is a big-bang cutover. The dual read is the mitigation.

## Findings at the time of the study

- **0.3.3 did not fix Codex seats.** Codex runs its commands and hooks under one shared app-server daemon: pid 5436, ppid 1, started from pane `%34` in `~/code/cofo-kb`, with `TMUX_PANE=%34`. The ancestor walk reaches pid 1 without finding a pane and falls back to `%34`. 0.3.4 (762204c) adds a fallback to the pane in the process's working directory. *(2026-10-06: not enough. See the revision: the daemon later ran as a pane's child, and 0.3.7 was needed.)*
- **Live state was corrupted.**
  - `identity/` registered murail-1b, murail-2b, murail-0b and cofo-kb-codex all on `%34`.
  - `@name=murail-1b` was set on both `%7` and `%34`.
  - A deliver hook that resolved to `%34` read murail-1b's inbox, which is the mechanism behind murail-m65jq.
- **The test instrument could not see the class.** In 0.3.3, `own_pane` ignored `HAIL_SOCKET`, so `test/run.sh` passed 20/36 inside tmux, and CI never runs under tmux. Fixed in 0.3.4, which also adds scenario 37.

## Data model today

message `inbox/<key>/<id>.md` · receipt `.read` sidecar (time | injected | inline) · envelope typed into the pane (headline cap 400, folded) · label = tmux `@name` + `identity/<label>` {pane, incarnation} · incarnation `incarnation/<pane>` · obligations `obligations/<key>/<id>` · holds `holds/<id>` · sent `sent/<key>/<id>` · read mark `read/<pane>`.

```
sender: hail read T -> hail T "h" --kind k --body B
  resolve label -> pane_guard (dialog/draft/colour) -> write inbox -> send-keys envelope,
  verify, Enter -> write holds/obligations/sent
recipient: UserPromptSubmit hook `hail deliver` -> self_key(TMUX_PANE -> label)
  -> write .read "injected" -> print JSON
SessionStart: hail brief (all holds + my obligations)
```

## Failure classes

| class | root cause | evidence |
|---|---|---|
| identity | Who you are comes from the inherited `TMUX_PANE` or a pane id; labels are not unique; seat.sh relabels on every start | 4vc8v, m65jq, the `%34` census; label hijack; the D1 hello overwrite |
| delivery | The content travels as keystrokes into a UI hail cannot see: vim mode, paste blocks, drafts, ghost text, the agent panel, dialogs | guard fixes cbdfe1a, 5fbe925, ee5f64d, 1a4c56d; cap churn 160→400→240→400 |
| sender shell | The message is a shell argument | a backticked `nix store gc` ran; apostrophes and `==` break sends |
| state | Nothing expires; stores are keyed by dead pane ids; the receipt is written before output; bead ids are auto-detected | 40 holds since 09-05; 93 open obligations for 1a; a 48.7 KB brief; `inbox/%1` holds 2306 files |
| concurrency | deliver checks, then writes, with no atomic claim; `send-keys` blocks on a wedged pty | double injection (m65jq); send-keys pile-ups |

## Invariants

1. **Addressing.** A message reaches exactly its addressee, whose identity cannot be inherited from another seat.
2. **Exactly-once claim and truthful receipt.** A receipt exists only after the claim.
3. **Composer integrity.** No sender's content ever reaches another composer as keystrokes.
4. **Liveness.** A working agent gets stop-class mail before its next tool call, and an idle agent is woken.
5. **Bounded context.** A few lines reach the prompt, and the body is fetched once.

These mechanisms exist only to work around typing into panes or pane-as-identity:
- the read guard;
- the draft and dialog guard, and the colour parser;
- typed-text verification and vim repair;
- the headline cap and fold;
- inline receipts;
- incarnations, hello, name and exit 3;
- the dual inbox;
- socket scanning;
- `own_pane`;
- the `tmux-bridge` alias.

## Recommended design

- **Identity is the seat.** A seat is the desk directory (`jj workspace root`), or a `.hail-seat` file. `HAIL_SEAT` is accepted only when it agrees with the seat derived from cwd; anything else refuses with a type. A pane is only a wake target: `@seat`, set by the launcher and cross-checked against `pane_current_path`.
- **A Maildir inbox:** `seats/<seat>/{tmp,new,cur}/<id>`.
  - A write lands in `tmp` and is renamed into `new`.
  - A reader claims a message by renaming it into `cur`.
  - If emitting fails, it renames the message back, so delivery is at least once, under a stable id.
  - The receipt is the file sitting in `cur`.
- **Transport is hooks plus a wake, and never typed content.**
  - The UserPromptSubmit, PostToolUse (mid-turn) and Stop hooks deliver.
  - An idle agent is woken by typing one constant token, and only into an empty composer.
  - Verify that Codex supports PostToolUse and Stop before relying on them.
- **Command surface:** `hail send <seat> [--kind] [--re] [--bead] <<'EOF' … EOF` (stdin body), `deliver --event`, `brief` (at most 10 lines), `inbox`, `show`, `sent`, `await`, `whoami`, `seats`.
- **Deleted:**
  - name, hello, who, resolve, id, own_pane;
  - incarnations and `identity/`;
  - read, type and keys, with the read guard;
  - the colour parser, send_text_verified, the cap and fold, inline kinds;
  - `.read` sidecars;
  - holds and obligations, which move to bd;
  - bead auto-detect, socket scanning, the `tmux-bridge` alias.
- **Migration:**
  1. v0.4 writes the new layout, and deliver reads both layouts for one week.
  2. seat.sh drops its labelling.
  3. The old tree is archived read-only.
  4. Morgan triages the 40 holds.

  The expected result is 1571 lines down to about 400.

## Test plan

- **The harness.**
  - It runs hermetically, under `env -i` with `TMUX_TMPDIR=$SCRATCH`.
  - CI runs it both bare and nested inside tmux.
  - A canary checks that the default socket fails.
- **Reds per class:**
  - Identity: a daemon child (done as scenario 37 in 0.3.4); two panes with the same `@seat` refuse the wake; a subagent in the same seat gets no second copy.
  - Delivery: fake composers never capture message bytes.
  - Sender shell: a body with backticks, `$()`, `'`, `==` and invalid UTF-8 arrives byte-identical.
  - State: a crash between claim and emit redelivers the message under the same id; the JSON parses for random bytes.
  - Concurrency: 1000 parallel sends collide on no id; a wedged pty returns within 2 s.
- **Properties:** a Hypothesis stateful model of send, claim, emit-fail, crash and await.

## Division of labour with jj-ops

- **Freeze receipts:** `just freeze` builds on `freeze-hash`, so no agent types an id.
- **Holds:** a bd gate bead, enforced by `just land`.
- **Land announcements:** `just land` appends to a feed, and deliver shows the unseen entries.
- **Obligations:** bd assignee and status.
- **Skills:** the two near-identical jj skills collapse to the generic one plus a short murail appendix.

## Plan

| # | slice | deletes |
|---|---|---|
| S1 | The hermetic harness and seat-from-cwd identity. seat.sh stops labelling. | own_pane, name/hello/who/resolve/id, identity/, incarnation/, the tmux-bridge alias |
| S2 | The Maildir inbox and the property suite | .read sidecars, receipt flavours, the dual inbox |
| S3 | Wake, not type: hooks and stdin bodies | send_text_verified, vim repair, the cap and fold, inline kinds, the read guard, most of pane_guard |
| S4 | State out: holds to bd, enforced by land; a brief of at most 10 lines | holds/, obligations/, the hold/block/release/done/announce kinds, bead auto-detect |
| S5 | jj integration: `just freeze`, the land feed, and the docs and skill rewritten | hand-typed receipts |
