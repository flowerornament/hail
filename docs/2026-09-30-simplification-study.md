# hail simplification study (2026-09-30)

A whole-system study of hail and the jj-ops workflow, requested by Morgan after a long run of hail defect fixes. It was read-only: sources, docs, skills, tracker, memory notes, and a live state and process census. murail-1a banked it. Citations are to `bin/hail` at 0.3.3 (d20de6b).

## Findings at the time of the study

- **0.3.3 did not fix Codex seats.** Codex runs its commands and hooks under one shared app-server daemon: pid 5436, ppid 1, started from pane `%34` in `~/code/cofo-kb`, with `TMUX_PANE=%34`. The ancestor walk reaches pid 1 without finding a pane and falls back to `%34`. 0.3.4 (762204c) adds a fallback to the pane in the process's working directory.
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
