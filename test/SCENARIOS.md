# tmux-bridge inbox — test log (2026-09-05)

Automated as `test/run.sh` (scenarios 1–15 below, plus await success/timeout/--any,
the `message`/`msg` aliases and the `tmux-bridge` symlink). Kept as the record of the
hand run that the harness reproduces.

Scratch server: `tmux -L tbtest` (socket /private/tmp/tmux-501/tbtest), session `t`,
pane %0 = `bash --norc` (sender, labeled `boss` then unlabeled), pane %1 = `cat`
(receiver, labeled `worker`). `TMUX_BRIDGE_SOCKET` pointed at that socket,
`XDG_STATE_HOME` at a scratch dir. Server killed at the end; the user's default
tmux server was never touched. `bd` (v1.0.5) is on PATH but no matching db/bead.

| # | Test | Result |
|---|------|--------|
| 1 | `message worker "<long text naming herald-ke7is>" --kind ruling --body -` run from inside pane %0 via send-keys | PASS: envelope `[tb kind:ruling from:boss/%0 reply:%0 id:.. bead:herald-ke7is] <ask…> — tmux-bridge inbox` landed in %1, exactly 160 chars; ask truncated with `…`; file `inbox/worker/<id>.md` has header + full ask + stdin body; stdout `id=<id>`; rc=0 |
| 2 | bd fallback (bd present, comment fails) | PASS: one stderr warning, file has `bead: herald-ke7is (not posted)`, message still delivered, rc=0 |
| 3 | `sent <id>` before any read | PASS: `delivered` |
| 4 | `inbox --peek` as %1 | PASS: prints header+body, no `.read` created, `sent` still `delivered` |
| 5 | `inbox` as %1 | PASS: prints, writes `<id>.read` with UTC time; `sent` -> `read 2026-09-05T06:01:19Z`; second `inbox` -> `(inbox empty)`; `--all` shows it again |
| 6 | `sent nope-0000` | PASS: `unknown` |
| 7 | `--kind stop` with 190-char text | PASS: full text typed inline, no `…`, no fetch hint, file still written |
| 8 | ask containing `herald-abc.2`, `tmux-bridge`, `read-only` | PASS: none treated as a bead (see deviation 1), plain envelope |
| 9 | `--bead murail-zz9zz --body file` | PASS: body from file, warning on bd failure, `bead:` in envelope |
| 10 | `--kind bogus` | PASS: error listing valid kinds, rc=1 |
| 11 | send to unlabeled %0 from labeled %1 | PASS: inbox dir `inbox/%0/`, `from: worker/%1`; `inbox` as %0 reads it |
| 12 | fake `bd` on PATH returning `{"id": 7}` | PASS: stdout `id=.. ` + `bead=murail-ke7is comment=7`; file `bead: murail-ke7is (comment 7)` + `see: bd show murail-ke7is` |
| 13 | PATH without bd, text names a bead, `--kind hold` | PASS: warning, file-only, rc=0 |
| 14 | `version`, `resolve`, `id`, `list`, `doctor`, help text | PASS: unchanged behaviour, help updated |
| 15 | `bash -n`; patch applies cleanly to `.orig` and reproduces the edited script | PASS |

Not tested: real `bd comment` against a live beads db (avoided writing to the user's db);
the send_text_verified repair path (unchanged code).
