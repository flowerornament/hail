#!/usr/bin/env bash
# Time hail's hot paths (deliver, brief, sent, whoami) with hyperfine against
# CPU budgets (user + system), on a scratch state root. CPU time, not wall
# time: the guard is about the tool's own cost, and a loaded host stretches
# wall time (scheduling) far more than CPU time. A run fails when the mean CPU
# time exceeds BENCH_FACTOR (default 3) times its budget.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
command -v hyperfine >/dev/null || { echo "bench: hyperfine not installed; skipped"; exit 0; }
cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml"
HAIL="$ROOT/target/release/hail"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/hail-bench.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/boss" "$WORK/worker" "$WORK/home"
echo boss > "$WORK/boss/.hail-seat"; echo worker > "$WORK/worker/.hail-seat"
export XDG_STATE_HOME="$WORK/state" HOME="$WORK/home" HAIL_SOCKET=/nonexistent/bench
unset TMUX TMUX_PANE
(cd "$WORK/worker" && "$HAIL" whoami >/dev/null)
id=$( (cd "$WORK/boss" && "$HAIL" worker ask 'bench' --no-wake 2>/dev/null || true) | sed -n 's/^id=//p')
factor="${BENCH_FACTOR:-3}"

# name, budget in ms, command (run in the worker's seat)
fail=0
while IFS='|' read -r name budget cmd; do
  mean=$(cd "$WORK/worker" && hyperfine -N --warmup 5 --runs 50 --export-json "$WORK/r.json" "$cmd" >/dev/null 2>&1 \
    && python3 -c 'import json,sys; r=json.load(open(sys.argv[1]))["results"][0]; print(round((r["user"]+r["system"])*1000, 2))' "$WORK/r.json")
  limit=$(python3 -c "print($budget * $factor)")
  verdict=ok
  python3 -c "import sys; sys.exit(0 if $mean <= $limit else 1)" || { verdict=SLOW; fail=1; }
  printf '%-6s %-24s %6s ms CPU  (budget %s ms, limit %s ms)\n' "$verdict" "$name" "$mean" "$budget" "$limit"
done <<EOF2
deliver, empty|2|$HAIL deliver --format claude
brief|3|$HAIL brief --hook
sent|2|$HAIL sent $id
whoami|2|$HAIL whoami
EOF2
exit "$fail"
