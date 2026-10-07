#!/usr/bin/env bash
# Add a jj workspace for one more writing agent: ../<name>, starting on main,
# with bd and Claude's memory pointing back at this checkout.
#
# Usage: scripts/workspace-add.sh <name>   (e.g. hail-2b: pair 2, implementer)
#
# Runs only from the colocated checkout, which owns .git and the bd database.
# Claude Code keys memory by directory (its absolute path with `/` and `.` as
# `-`, under $CLAUDE_CONFIG_DIR/projects, default ~/.claude), and a workspace
# would otherwise start with none.
set -euo pipefail

name="${1:-}"
[[ "$name" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "usage: workspace-add.sh <name>, e.g. hail-2b" >&2; exit 64; }
root="$(cd "$(dirname "$0")/.." && pwd)"
[ -d "$root/.git" ] || { echo "workspace-add: run this from the colocated checkout, not a jj workspace" >&2; exit 1; }
dest="$(dirname "$root")/$name"
[ ! -e "$dest" ] || { echo "workspace-add: $dest already exists" >&2; exit 1; }

(cd "$root" && jj workspace add --name "$name" -r main "$dest")
mkdir -p "$dest/.beads"
printf '../%s/.beads\n' "${root##*/}" > "$dest/.beads/redirect"

projects="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects"
key() { printf '%s' "$1" | tr '/.' '--'; }
memory="$projects/$(key "$root")/memory"
link="$projects/$(key "$dest")/memory"
if [ -d "$memory" ] && [ ! -e "$link" ]; then
  mkdir -p "${link%/*}"
  ln -s "$memory" "$link"
fi
echo "workspace-add: $dest is ready; start its agent there"
