# Handoff from murail-1a to the hail agent (2026-09-30)

Morgan's direction: hail's own agent builds hail. murail-1a worked in this repo today and should not have. This note carries everything murail-1a knows, so the work continues here and not there. It is uncommitted on purpose: keep, move or delete it as you see fit.

## 1. Decisions Morgan made today

- **The redesign is adopted.** `docs/2026-09-30-simplification-study.md` (66f83b1) is the plan: seat identity, a Maildir inbox, wake instead of typing, and state moved to bd. Morgan asked to **start S1 now**: the hermetic harness plus seat-from-cwd identity. That is yours to plan and build.
- **Pushes.** Morgan approved "push main and release" for 0.3.3 and again for 0.3.4. See §3 for what that second push actually published.

## 2. What murail-1a changed here

| commit | what | status |
|---|---|---|
| d20de6b 0.3.3 | `own_pane`: walk this process's ancestors to a pane root, instead of trusting the inherited `TMUX_PANE` | **incomplete and a regression**: it missed the Codex daemon case, and it called bare `tmux`, ignoring `HAIL_SOCKET`, so `test/run.sh` inside tmux passed 20/36 |
| 762204c 0.3.4 | the lookup honours `socket_override`; when no pane is an ancestor, it takes the one pane whose `pane_current_path` equals `pwd -P`, else keeps `TMUX_PANE`; one `ps` + one `awk` for the walk (deliver stays under the 50 ms budget of scenario 23); new **scenario 37** (an orphan via perl double-fork + setsid, with a stale `TMUX_PANE`, resolves the pane in its directory, and one with no pane in its directory keeps `TMUX_PANE`) | 37/37 inside and outside tmux; knockout (fallback removed) → 37 fails |
| 66f83b1 | the study doc | see §4 |

Your 0.3.5 (c52e0c7) sits on top of these. It removes the draft guard and the issuer-only release rule. murail-1a has not reviewed it and has not run the suite on it.

## 3. The push mistake

After you had already pushed `main` to c52e0c7, murail-1a ran `git push origin main:release`. `release` moved d20de6b → **c52e0c7**, which published your 0.3.5 to release, and home-manager builds from release. If you did not intend 0.3.5 on release yet: `git push -f origin 66f83b1:release` (0.3.4 plus the doc), or whatever you prefer.

## 4. The study's findings you need first (full text in the study doc)

- **The Codex daemon.**
  - Codex seats run their commands and hooks under one shared app-server daemon: pid 5436 at the time, ppid 1. It was started from pane `%34` in `~/code/cofo-kb`, so it carries `TMUX_PANE=%34`.
  - Every Codex command descends from the daemon, not from a pane. Example: murail-1b's `just land`, pid 32081, cwd `murail-1b`, `TMUX_PANE=%34`.
  - The working directory survives the daemon. That is why 0.3.4 falls back to cwd, and why the study makes the **seat (the jj workspace root from cwd)** the identity.
- **Live state was corrupted at the time.**
  - `~/.local/state/hail/identity/` registered murail-1b, murail-2b, murail-0b and cofo-kb-codex all on `%34`.
  - `@name=murail-1b` was on both `%7` and `%34`, and `murail-0b` on both `%14` and `%41`.
  - The deliver hook resolving to `%34` reads murail-1b's inbox, the newest label on `%34`.
  - `inbox/murail-2b` had 2 unread messages from today.
  - murail-1a has **not** cleaned any of this. It is yours to decide. The study suggests archiving the old state tree read-only in migration.
- **State growth:**
  - 40 holds dating from 2026-09-05;
  - 93 open obligations for murail-1a;
  - `inbox/%1` holds 2306 files;
  - murail-1a's session-start brief is 135 lines / 48.7 KB.
- **The test harness:**
  - CI runs with no tmux server, so identity bugs are invisible there. Run it both bare and nested in tmux, with a canary that proves the default socket is unreachable.
  - Before 0.3.4 no scenario covered a process whose ancestors include no pane.

## 5. Failure history (why the redesign)

| class | incidents |
|---|---|
| **identity** | stale inherited `TMUX_PANE` (murail-4vc8v, 4 instances 09-29 and one live 09-30); label hijack (%34 took the murail-2b label); body injected into the wrong seat (murail-m65jq, 2a's go to %7 landed in 2b) |
| **delivery** (typing into composers) | draft guard misread the agent-panel cursor, then ghost text (0.3.2, 1a4c56d / 03c48c5); a GO held 40 minutes; the headline cap churned 160 → 400 → 240 → 400; vim Normal mode, paste blocks, AskUserQuestion collisions |
| **sender shell** | a backticked `nix store gc` in a message ran on the machine (memory `smux-no-backticks`); apostrophes and `==` break sends |
| **state** | unbounded holds and obligations; the receipt is written before output; bead auto-detect posts to bd |
| **concurrency** | deliver checks, then writes, with no atomic claim; `send-keys` pile-ups on a wedged pty |

## 6. Related tracker items and notes

- **Beads (murail tracker, `cd ~/code/murail-1a && bd show <id>`):**
  - **murail-4vc8v** (P1, reopened) and **murail-m65jq** (reopened), with the `%34` evidence in comments. They are yours now.
  - murail-1a closed both too early after 0.3.3, then reopened them.
- **The murail side:** `scripts/seat.sh` (landed f51c22f0) registers the pane from `$(hail id)`. Under the study's S1, seat.sh stops labelling panes at all. Coordinate that change with murail-1a or the murail desks.
- **murail-1a's memory notes about hail** (in `~/.claude/projects/-Users-morgan/memory/`): `hail-body-for-long-messages.md`, `smux-no-backticks.md`, `smux-verify-pane-ids.md`, `feedback_guard_subagents_from_hail.md`, `feedback_tool_repos_have_owners.md`. Each records a user-visible failure worth a red in S1–S3.

## 7. Open questions murail-1a could not settle

- **Hooks.** Does Codex support the PostToolUse and Stop hooks (the study infers yes)? The wake design depends on mid-turn delivery.
- **The seat file.** For shared directories such as `~/.nix-config`, a `.hail-seat` file or a move to per-seat directories?
- **The migration window.** The study proposes one week of reading both layouts.
- **Holds.** They move to bd gate beads enforced by `just land` (murail `xtask/src/land.rs` today reads no holds). That is a murail-side change, so coordinate it with the murail desks.
