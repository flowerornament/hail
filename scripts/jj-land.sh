#!/usr/bin/env bash
# Publish the described jj change: gate exactly it, then push exactly it.
#
# Usage: scripts/jj-land.sh [bookmark] [--dry-run]
#   The bookmark defaults to LAND_BOOKMARK, else `main` if it exists, else `master`.
#
# This is the only publish verb in a jj repository whose gates used to run in
# git hooks: jj runs no hooks, so the gate travels with the publish instead.
# It pins the candidate commit before the gate, runs the gate, checks that
# nothing moved, and only then moves the bookmark to the pinned commit and
# pushes it. It rewrites nothing, so the gated bytes are the published bytes.
# A raw `jj git push` skips the gate; do not use one.
#
# The candidate is `@` when `@` has changes, otherwise `@-`. Everything
# between the bookmark and the candidate is published with it; the gate runs
# on the candidate's tree, which includes them all. It refuses:
#   - nothing to publish (the candidate is empty or already in the bookmark);
#   - any undescribed commit in the range it would publish;
#   - a candidate not on top of the bookmark (rebase first);
#   - a local bookmark that differs from the remote one (fetch first);
#   - `@-` as candidate while `@` holds uncommitted edits;
#   - any change to the working copy or the bookmark while the gate ran.
# The gate must write only ignored paths: a file it leaves behind changes `@`.
# If the push fails, the bookmark moves back, so a later land starts clean.
#
# The gate command is LAND_GATE (default `just check`). --dry-run runs the
# gate and every check, then stops before publishing.
set -euo pipefail

bookmark=""
dry_run=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    -*) echo "usage: jj-land.sh [bookmark] [--dry-run]" >&2; exit 64 ;;
    *) bookmark="$arg" ;;
  esac
done
if [ -z "$bookmark" ]; then
  bookmark="${LAND_BOOKMARK:-}"
  if [ -z "$bookmark" ]; then
    if [ -n "$(jj log --no-graph -r 'present(main)' -T commit_id 2>/dev/null)" ]; then
      bookmark=main
    else
      bookmark=master
    fi
  fi
fi
gate="${LAND_GATE:-just check}"
remote="${LAND_REMOTE:-origin}"

refuse() { echo "land: refused: $*" >&2; exit 1; }

# One commit id for a revset, or empty when it names none. The first call in
# an observation snapshots the working copy, so later calls see current files.
rev() { jj log --no-graph -r "$1" -T 'commit_id ++ "\n"' 2>/dev/null | head -1; }

observe() {
  at="$(rev '@')"
  if [ -n "$(rev '@ & ~empty()')" ]; then
    candidate="$at"
  else
    candidate="$(rev '@- & ~empty()')"
  fi
  target="$(rev "present($bookmark)")"
  remote_tip="$(rev "present($bookmark@$remote)")"
}

observe
[ -n "$candidate" ] || refuse "nothing to publish: @ and @- are empty"
[ -n "$target" ] || refuse "bookmark $bookmark does not exist"
[ -n "$(rev "$candidate & ::$bookmark")" ] &&
  refuse "nothing to publish: ${candidate:0:12} is already in $bookmark"
[ -n "$(rev "$bookmark & ::$candidate")" ] ||
  refuse "$bookmark is not an ancestor of ${candidate:0:12}; run: jj rebase -d $bookmark"
undescribed="$(rev "$bookmark..$candidate & description(exact:'')")"
[ -z "$undescribed" ] ||
  refuse "${undescribed:0:12} has no description; run: jj describe -r ${undescribed:0:12} -m \"area: subject\""
if [ -n "$remote_tip" ] && [ "$remote_tip" != "$target" ]; then
  refuse "$bookmark differs from $bookmark@$remote; run: jj git fetch, then jj rebase -d $bookmark"
fi
if [ "$candidate" != "$at" ] && [ -n "$(jj diff --from "$candidate" --to @ --summary)" ]; then
  refuse "@ holds edits on top of ${candidate:0:12}; describe them into the change or move them off"
fi

pinned_candidate="$candidate" pinned_at="$at" pinned_target="$target" pinned_remote="$remote_tip"
count="$(jj log --no-graph -r "$bookmark..$candidate" -T '"x\n"' | grep -c x || true)"
echo "land: pinned ${pinned_candidate:0:12} for $bookmark (on ${pinned_target:0:12}, $count commit(s)); gate: $gate"

if ! bash -c "$gate"; then
  refuse "gate failed ($gate); nothing published"
fi

observe
if [ "$at" != "$pinned_at" ] || [ "$candidate" != "$pinned_candidate" ]; then
  jj diff --from "$pinned_at" --to "$at" --summary >&2 || true
  refuse "the working copy changed while the gate ran (above); land again"
fi
[ "$target" = "$pinned_target" ] && [ "$remote_tip" = "$pinned_remote" ] ||
  refuse "$bookmark moved while the gate ran; run: jj git fetch, jj rebase -d $bookmark, land again"

if [ "$dry_run" = 1 ]; then
  echo "land: ADMITTED ${pinned_candidate:0:12} -> $bookmark (dry run; nothing published)"
  exit 0
fi

jj bookmark move "$bookmark" --to "$pinned_candidate"
if ! jj git push --remote "$remote" --bookmark "$bookmark"; then
  jj bookmark move "$bookmark" --to "$pinned_target" --allow-backwards
  refuse "push failed; $bookmark moved back to ${pinned_target:0:12}; land again"
fi
echo "land: PUBLISHED ${pinned_candidate:0:12} -> $bookmark"
