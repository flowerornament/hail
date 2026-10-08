#!/usr/bin/env bash
# hail test harness — every scenario against a scratch tmux server (-L
# hailtest-<pid>) and a scratch state root. Never touches the default tmux server,
# the real state or the real hooks: tmux calls name the scratch socket, hail
# gets HAIL_SOCKET, XDG_STATE_HOME and HOME under $SCRATCH.
#
# Each pane runs in its own seat directory ($SCRATCH/boss, $SCRATCH/worker),
# and `as <pane>` runs hail from that pane's directory, as an agent would.
# HAIL_BIN selects the binary (default: target/debug/hail; `just test` builds it).
# shellcheck disable=SC1010,SC2034,SC2010  # 'done' is a hail kind; loop counters; ls|grep counts
set -uo pipefail
# Heredocs inside $( ) need bash 4+; macOS ships 3.2 as /bin/bash.
(( BASH_VERSINFO[0] >= 4 )) || { echo "test/run.sh needs bash 4 or later (this is $BASH_VERSION)"; exit 2; }

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
HAIL="${HAIL_BIN:-$HERE/../target/debug/hail}"
[[ -x "$HAIL" ]] || { echo "no hail binary at $HAIL (cargo build, or set HAIL_BIN)"; exit 2; }
# The version under test is the one Cargo.toml declares; releases bump that line.
HAIL_VERSION=$(sed -n 's/^version = "\([0-9.]*\)"$/\1/p' "$HERE/../Cargo.toml" | head -1)
[[ -n "$HAIL_VERSION" ]] || { echo "cannot read the version from Cargo.toml"; exit 2; }
# One server per run: two jj workspaces may run `just check` at once, and a
# shared server name lets each run kill or reuse the other's windows.
SOCKNAME="hailtest-$$"
T=(tmux -L "$SOCKNAME")

unset TMUX TMUX_PANE HAIL_SOCKET TMUX_BRIDGE_SOCKET HAIL_SEAT CLAUDE_CONFIG_DIR CODEX_HOME

SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/hailtest.XXXXXX")
export HAIL_ENVELOPE_MAX=160   # scenarios were written against the original cap; the tool default is 400
export XDG_STATE_HOME="$SCRATCH/state"
export HOME="$SCRATCH/home"
# The scratch panes run bash and cat; count them as agents.
export HAIL_AGENT_COMMANDS="bash,cat"
STATE="$XDG_STATE_HOME/hail"
SEATS="$STATE/seats"
BOSS_DIR="$SCRATCH/boss"; WORKER_DIR="$SCRATCH/worker"
mkdir -p "$HOME" "$BOSS_DIR" "$WORKER_DIR"
echo boss > "$BOSS_DIR/.hail-seat"; echo worker > "$WORKER_DIR/.hail-seat"

cleanup() {
  "${T[@]}" kill-server 2>/dev/null || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

# --- bd tripwire ---------------------------------------------------------------
# hail never runs bd. A bd first on PATH records any call (scenario 2 checks
# for none), and keeps the real bd and the user's beads db out of reach.
mkdir -p "$SCRATCH/bd-trap" "$SCRATCH/bin"
printf '#!/bin/sh\necho "$@" >> "%s/bd-called"\nexit 1\n' "$SCRATCH" > "$SCRATCH/bd-trap/bd"
chmod +x "$SCRATCH/bd-trap/bd"
ln -s "$HAIL" "$SCRATCH/bin/tmux-bridge"
ln -s "$HAIL" "$SCRATCH/bin/hail"
BASE_PATH="$PATH"
export PATH="$SCRATCH/bd-trap:$BASE_PATH"

# --- scratch server ----------------------------------------------------------
"${T[@]}" kill-server 2>/dev/null || true
"${T[@]}" -f /dev/null new-session -d -s t -x 300 -y 50 -c "$BOSS_DIR" 'bash --norc' || { echo "cannot start scratch tmux server"; exit 2; }
"${T[@]}" split-window -t t -d -c "$WORKER_DIR" cat
PANES=$("${T[@]}" list-panes -t t -F '#{pane_id}')
SENDER=$(printf '%s\n' "$PANES" | sed -n 1p); RECV=$(printf '%s\n' "$PANES" | sed -n 2p)
export HAIL_SOCKET
HAIL_SOCKET=$("${T[@]}" display-message -p '#{socket_path}')
[[ -S "$HAIL_SOCKET" ]] || { echo "scratch socket missing: $HAIL_SOCKET"; exit 2; }
sleep 0.3

# --- helpers -----------------------------------------------------------------
PASS=0; FAIL=0; FAILED=()
dir_of() { case "$1" in "$SENDER") echo "$BOSS_DIR" ;; "$RECV") echo "$WORKER_DIR" ;; *) echo "$SCRATCH" ;; esac; }
as() { local pane="$1"; shift; (cd "$(dir_of "$pane")" && TMUX_PANE="$pane" "$HAIL" "$@" </dev/null); }
pane_text() { "${T[@]}" capture-pane -t "$1" -p -J; }
# Each scenario starts with no hooked mark, so whether an fyi is typed never
# depends on which scenarios ran before it.
reset_recv() { "${T[@]}" respawn-pane -k -c "$WORKER_DIR" -t "$RECV" cat; rm -f "$SEATS"/*/hooked; sleep 0.2; }
# respawn with a command that prints something first, then behaves like cat
recv_showing() { "${T[@]}" respawn-pane -k -c "$WORKER_DIR" -t "$RECV" bash -c "printf '%s\n' \"\$@\"; exec cat" _ "$@"; sleep 0.3; }
reset_sender() { "${T[@]}" send-keys -t "$SENDER" C-c; rm -f "$SEATS"/*/hooked; sleep 0.1; }
# send FROM TARGET args... : stdout/stderr/rc land in $OUT/$ERR/$RC. No read
# first: 0.4 sends need none.
OUT=""; ERR=""; RC=0
send() {
  local from="$1" target="$2"; shift 2
  OUT=$(as "$from" send "$target" "$@" 2>"$SCRATCH/err"); RC=$?
  ERR=$(cat "$SCRATCH/err")
}
# The message file for an id, wherever it is (new/ or cur/), in any seat.
msgfile() { ls "$SEATS"/*/new/"$1".md "$SEATS"/*/cur/"$1".*.md 2>/dev/null | head -1; }
unread_file() { echo "$SEATS/$2/new/$1.md"; }
last_id() { printf '%s\n' "$OUT" | sed -n 's/^id=//p' | head -1; }
envelope_line() { pane_text "$RECV" | grep -F "$1" | head -1 | sed 's/[[:space:]]*$//'; }
chars() { printf '%s' "$1" | LC_ALL=en_US.UTF-8 wc -m | tr -d ' '; }
expect() { # expect "description" check-fn args...
  local what="$1"; shift
  if ! "$@"; then echo "    - $what"; return 1; fi
}
contains() { [[ "$1" == *"$2"* ]]; }
not_contains() { [[ "$1" != *"$2"* ]]; }
eq() { [[ "$1" == "$2" ]]; }
re() { [[ "$1" =~ $2 ]]; }
exists() { [[ -e "$1" ]]; }
missing() { [[ ! -e "$1" ]]; }
empty() { [[ -z "$1" ]]; }
le() { (( $1 <= $2 )); }
alive() { kill -0 "$1" 2>/dev/null; }
wait_for_file() { local f="$1" _; for _ in $(seq 1 100); do [[ -e "$f" ]] && return 0; sleep 0.1; done; return 1; }

scenario() { # scenario N "title" fn
  local n="$1" title="$2" fn="$3"
  if "$fn"; then PASS=$((PASS+1)); printf 'PASS %2s  %s\n' "$n" "$title"
  else FAIL=$((FAIL+1)); FAILED+=("$n"); printf 'FAIL %2s  %s\n' "$n" "$title"; fi
}

# --- scenarios ---------------------------------------------------------------

LONG_ASK='Ruling on herald-ke7is: convert at the receipt, not the producer; the fan-in gate hashes checkout contents so scratch state must stay outside every repo tree, and the coordinator should not read panes for replies'
ASK1='Ruling on herald-ke7is: convert at the receipt, not the producer; scratch state stays outside every repo tree; do not read panes for replies'

s1() { # from inside the sender pane via send-keys, --kind ruling --body -
  printf 'line one of the body\nline two of the body\n' > "$SCRATCH/body1"
  local ok=0
  # The pane runs the script so TMUX_PANE comes from tmux itself, not from
  # the harness. A short typed line: readline stalls on lines past ~1 KB.
  cat > "$SCRATCH/s1.sh" <<EOS
export HAIL_ENVELOPE_MAX='$HAIL_ENVELOPE_MAX' HAIL_SOCKET='$HAIL_SOCKET' XDG_STATE_HOME='$XDG_STATE_HOME' PATH='$PATH' HOME='$HOME' HAIL_AGENT_COMMANDS='$HAIL_AGENT_COMMANDS'
'$HAIL' send worker '$ASK1' --kind ruling --body - <'$SCRATCH/body1' >'$SCRATCH/s1.out' 2>'$SCRATCH/s1.err'
echo \$? >'$SCRATCH/s1.rc'
EOS
  "${T[@]}" send-keys -t "$SENDER" -l -- ". '$SCRATCH/s1.sh'; clear"
  "${T[@]}" send-keys -t "$SENDER" Enter
  expect "sender finished" wait_for_file "$SCRATCH/s1.rc" || return 1
  sleep 0.3
  OUT=$(cat "$SCRATCH/s1.out"); ERR=$(cat "$SCRATCH/s1.err"); RC=$(cat "$SCRATCH/s1.rc")
  local id line
  id=$(last_id)
  line=$(envelope_line "id:$id")
  expect "rc=0 (got $RC)" eq "$RC" 0 || ok=1
  expect "stdout is id=<id>" re "$OUT" '^id=[0-9]{4}T[0-9]{6}-[0-9a-f]{4}$' || ok=1
  expect "envelope head" contains "$line" "[hail kind:ruling from:boss/$SENDER reply:boss id:$id]" || ok=1
  expect "fetch hint" contains "$line" "— hail inbox" || ok=1
  expect "headline typed in full" contains "$line" "] $ASK1 — hail inbox" || ok=1
  expect "not truncated" not_contains "$line" "…" || ok=1
  expect "submitted: cat echoed the envelope" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  local f; f=$(unread_file "$id" worker)
  expect "inbox file exists" exists "$f" || ok=1
  expect "file has full ask" grep -qF "ask: $ASK1" "$f" || ok=1
  expect "file has stdin body" grep -qF "line two of the body" "$f" || ok=1
  expect "file header from:" grep -qF "from: boss/$SENDER" "$f" || ok=1
  S1_ID="$id"
  reset_recv
  return "$ok"
}

s2() { # hail never runs bd: --bead is ignored with a notice, no bead: anywhere
  local ok=0 id
  rm -f "$SCRATCH/bd-called"
  send "$SENDER" worker "please look at herald-ke7is again" --kind ask --bead herald-ke7is
  id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "--bead noticed as ignored" contains "$ERR" "--bead is ignored" || ok=1
  expect "no bead: in the file" not_contains "$(cat "$(msgfile "$id")")" "bead:" || ok=1
  expect "no bead: in the envelope" not_contains "$(envelope_line "id:$id")" "bead:" || ok=1
  expect "bd never ran" missing "$SCRATCH/bd-called" || ok=1
  reset_recv; return "$ok"
}

s3() { eq "$(as "$SENDER" sent "$S1_ID")" delivered; }

s4() { # inbox --peek
  local ok=0 out
  out=$(as "$RECV" inbox --peek)
  expect "peek prints header" contains "$out" "from: boss/$SENDER" || ok=1
  expect "peek prints body" contains "$out" "line two of the body" || ok=1
  expect "still unread" exists "$(unread_file "$S1_ID" worker)" || ok=1
  expect "sent still delivered" eq "$(as "$SENDER" sent "$S1_ID")" delivered || ok=1
  return "$ok"
}

s5() { # inbox writes receipts
  local ok=0 out
  out=$(as "$RECV" inbox)
  expect "prints s1" contains "$out" "id: $S1_ID" || ok=1
  expect "claimed as read" exists "$SEATS/worker/cur/$S1_ID.read.md" || ok=1
  expect "sent -> read <time>" re "$(as "$SENDER" sent "$S1_ID")" '^read [0-9]{4}-.*Z$' || ok=1
  expect "second inbox empty" eq "$(as "$RECV" inbox)" "(inbox empty)" || ok=1
  expect "--all shows it again" contains "$(as "$RECV" inbox --all)" "id: $S1_ID" || ok=1
  return "$ok"
}

s6() { [[ "$(as "$SENDER" sent nope-0000)" == unknown ]]; }

s7() { # --kind stop, 120 chars typed inline
  local ok=0 text line id
  text="STOP: red gate on master, do not land anything until the fan-in gate is green again; details on the bead, ask boss ok"
  send "$SENDER" worker "$text" --kind stop
  id=$(last_id)
  line=$(envelope_line "id:$id")
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "full text inline" contains "$line" "] $text" || ok=1
  expect "no …" not_contains "$line" "…" || ok=1
  expect "no fetch hint" not_contains "$line" "hail inbox" || ok=1
  expect "claimed inline at send" exists "$SEATS/worker/cur/$id.inline.md" || ok=1
  expect "sent -> inline <time>" re "$(as "$SENDER" sent "$id")" '^inline [0-9]{4}-.*Z$' || ok=1
  expect "deliver does not hand it over again" not_contains "$(as "$RECV" deliver)" "id: $id" || ok=1
  expect "inbox does not either" not_contains "$(as "$RECV" inbox)" "id: $id" || ok=1
  expect "inbox --all shows the inline receipt" contains "$(as "$RECV" inbox --all)" "receipt: inline " || ok=1
  expect "brief does not list it as unread" not_contains "$(as "$RECV" brief)" "id:$id" || ok=1
  expect "await sees it" contains "$(as "$SENDER" await "$id" --timeout 1)" "$id inline " || ok=1
  reset_recv; return "$ok"
}

s9() { # --body file
  local ok=0 id line
  printf 'body from a file\n' > "$SCRATCH/body9"
  send "$SENDER" worker "ruling attached" --kind ruling --body "$SCRATCH/body9"
  id=$(last_id); line=$(envelope_line "id:$id")
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "body from file" grep -qF "body from a file" "$(msgfile "$id")" || ok=1
  expect "fetch hint" contains "$line" "] ruling attached — hail inbox" || ok=1
  reset_recv; return "$ok"
}

s10() { # --kind bogus
  local ok=0
  send "$SENDER" worker "hello" --kind bogus
  expect "rc=1" eq "$RC" 1 || ok=1
  expect "lists kinds" contains "$ERR" "ruling go nogo ask fyi done stop hold block release announce" || ok=1
  return "$ok"
}

s11() { # a pane id as target resolves to the seat in that pane's directory
  local ok=0 id
  send "$RECV" "$SENDER" "back at you" --kind fyi
  id=$(last_id)
  expect "rc=0 ($ERR)" eq "$RC" 0 || ok=1
  expect "in boss's inbox" exists "$(unread_file "$id" boss)" || ok=1
  expect "from: worker/$RECV" grep -qF "from: worker/$RECV" "$(msgfile "$id")" || ok=1
  expect "reply: worker" grep -qxF "reply: worker" "$(msgfile "$id")" || ok=1
  expect "envelope in sender pane" contains "$(pane_text "$SENDER")" "id:$id" || ok=1
  expect "inbox as boss reads it" contains "$(as "$SENDER" inbox)" "id: $id" || ok=1
  reset_sender
  return "$ok"
}

s14() { # version resolve id list doctor help
  local ok=0
  expect "version" eq "$(as "$SENDER" version)" "hail $HAIL_VERSION" || ok=1
  expect "--version" eq "$("$HAIL" --version)" "hail $HAIL_VERSION" || ok=1
  expect "-V" eq "$("$HAIL" -V)" "hail $HAIL_VERSION" || ok=1
  expect "version --json" eq "$("$HAIL" version --json)" "{\"name\":\"hail\",\"version\":\"$HAIL_VERSION\"}" || ok=1
  expect "help topic" contains "$("$HAIL" help kinds)" "ruling  go  ask" || ok=1
  expect "command --help" contains "$("$HAIL" send --help)" "--re <id>" || ok=1
  expect "unknown topic exits 1" eq "$("$HAIL" help bogus >/dev/null 2>&1; echo $?)" "1" || ok=1
  expect "no args prints the map" contains "$("$HAIL")" "hail — messages between coding agents" || ok=1
  expect "resolve worker" eq "$(as "$SENDER" resolve worker)" "$RECV" || ok=1
  expect "id" eq "$(as "$SENDER" id)" "$SENDER" || ok=1
  expect "list shows the seat" contains "$(as "$SENDER" list)" "worker" || ok=1
  expect "doctor names the seat" contains "$(as "$SENDER" doctor)" "seat here: boss" || ok=1
  expect "doctor rc=0" eq "$(as "$SENDER" doctor >/dev/null 2>&1; echo $?)" 0 || ok=1
  expect "help mentions await" contains "$("$HAIL" --help)" "await <id>..." || ok=1
  expect "help has no tmux-bridge text" not_contains "$("$HAIL" --help | grep -v TMUX_BRIDGE_SOCKET)" "tmux-bridge" || ok=1
  return "$ok"
}

s15() { # the seat is the directory: whoami; outside a seat, exit 3 with the fix
  local ok=0 out
  expect "whoami boss" contains "$(as "$SENDER" whoami)" "seat: boss" || ok=1
  expect "whoami worker" contains "$(as "$RECV" whoami)" "seat: worker" || ok=1
  mkdir -p "$SCRATCH/nowhere"
  out=$(cd "$SCRATCH/nowhere" && "$HAIL" whoami 2>&1); RC=$?
  expect "no seat rc=3 (got $RC)" eq "$RC" 3 || ok=1
  expect "no seat names the fix" contains "$out" ".hail-seat" || ok=1
  expect "HAIL_SEAT counts where no directory names a seat" contains "$(cd "$SCRATCH/nowhere" && HAIL_SEAT=loose "$HAIL" whoami)" "seat: loose" || ok=1
  mkdir -p "$SCRATCH/nowhere2"
  expect "HAIL_SEAT works from a second seatless directory" contains "$(cd "$SCRATCH/nowhere2" && HAIL_SEAT=loose "$HAIL" whoami)" "seat: loose" || ok=1
  expect "HAIL_SEAT cannot override the directory" contains "$(cd "$WORKER_DIR" && HAIL_SEAT=boss "$HAIL" whoami)" "seat: worker" || ok=1
  return "$ok"
}

s16() { # await success: blocks until the recipient runs inbox
  local ok=0 id out rc
  send "$SENDER" worker "please ack" --kind ask
  id=$(last_id)
  as "$SENDER" await "$id" --timeout 20 > "$SCRATCH/await16" 2>&1 &
  local pid=$!
  sleep 1.5
  expect "still blocked before inbox" alive "$pid" || ok=1
  as "$RECV" inbox >/dev/null
  wait "$pid"; rc=$?
  out=$(cat "$SCRATCH/await16")
  expect "rc=0 (got $rc)" eq "$rc" 0 || ok=1
  expect "prints '<id> read <time>'" re "$out" "^$id read [0-9]{4}-.*Z$" || ok=1
  reset_recv; return "$ok"
}

s17() { # await timeout; needs no tmux server
  local ok=0 id out rc start end
  send "$SENDER" worker "never read" --kind ask
  id=$(last_id)
  start=$(date +%s)
  out=$(HAIL_SOCKET=/nonexistent/socket as "$SENDER" await "$id" --timeout 1); rc=$?
  end=$(date +%s)
  expect "rc=1 (got $rc)" eq "$rc" 1 || ok=1
  expect "prints '<id> timeout'" eq "$out" "$id timeout" || ok=1
  expect "returned promptly" le $(( end - start )) 3 || ok=1
  reset_recv; return "$ok"
}

s18() { # await --any
  local ok=0 id out rc
  send "$SENDER" worker "read this one" --kind ask
  id=$(last_id)
  as "$RECV" inbox >/dev/null  # reads both s17 and this one
  send "$SENDER" worker "leave this one" --kind ask
  local unread; unread=$(last_id)
  out=$(as "$SENDER" await "$id" "$unread" --any --timeout 5); rc=$?
  expect "--any rc=0 (got $rc)" eq "$rc" 0 || ok=1
  expect "read id line" contains "$out" "$id read " || ok=1
  expect "unread id pending" contains "$out" "$unread pending" || ok=1
  out=$(as "$SENDER" await "$id" "$unread" --timeout 1); rc=$?
  expect "all-semantics rc=1 (got $rc)" eq "$rc" 1 || ok=1
  expect "unread id timeout" contains "$out" "$unread timeout" || ok=1
  reset_recv; return "$ok"
}

s19() { # alias message / msg
  local ok=0 id
  OUT=$(as "$SENDER" message worker "via alias" --kind fyi 2>/dev/null); RC=$?
  id=$(last_id)
  expect "message alias rc=0" eq "$RC" 0 || ok=1
  expect "delivered" exists "$(unread_file "$id" worker)" || ok=1
  OUT=$(as "$SENDER" msg worker "via msg" --kind fyi 2>/dev/null); RC=$?
  expect "msg alias rc=0" eq "$RC" 0 || ok=1
  reset_recv; return "$ok"
}

s20() { # tmux-bridge symlink + TMUX_BRIDGE_SOCKET fallback
  local ok=0 out
  expect "symlink version" eq "$("$SCRATCH/bin/tmux-bridge" version)" "hail $HAIL_VERSION" || ok=1
  out=$(env -u HAIL_SOCKET TMUX_BRIDGE_SOCKET="$HAIL_SOCKET" TMUX_PANE="$SENDER" "$SCRATCH/bin/tmux-bridge" resolve worker)
  expect "TMUX_BRIDGE_SOCKET fallback" eq "$out" "$RECV" || ok=1
  as "$SENDER" keys worker Escape >/dev/null 2>&1 || true   # consume any standing read mark
  expect "read guard error names hail read" contains "$(as "$SENDER" type worker x 2>&1)" "hail read" || ok=1
  return "$ok"
}

# --- 0.2.3 scenarios ----------------------------------------------------------
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
# Drop holds, obligations and send records left by earlier scenarios.
clear_state() { rm -rf "$STATE/holds" "$SEATS"/*/owed "$SEATS"/*/pending; }

s21() { # deliver: plain text, receipt says injected, silent afterwards, inbox --all distinguishes
  local ok=0 a b out
  printf 'body one\n' > "$SCRATCH/b21a"; printf 'second for deliver body\n' > "$SCRATCH/b21b"
  send "$SENDER" worker "first for deliver" --kind ask --body "$SCRATCH/b21a"; a=$(last_id)
  send "$SENDER" worker "second for deliver" --kind fyi --body "$SCRATCH/b21b"; b=$(last_id)
  out=$(as "$RECV" deliver); RC=$?
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "prints first body" contains "$out" "id: $a" || ok=1
  expect "prints second body" contains "$out" "second for deliver body" || ok=1
  expect "bodies separated" contains "$out" "---" || ok=1
  expect "claimed as injected" exists "$SEATS/worker/cur/$a.injected.md" || ok=1
  expect "sent -> injected <time>" re "$(as "$SENDER" sent "$a")" '^injected [0-9]{4}-.*Z$' || ok=1
  out=$(as "$RECV" deliver); RC=$?
  expect "second deliver silent" empty "$out" || ok=1
  expect "second deliver rc=0" eq "$RC" 0 || ok=1
  out=$(as "$RECV" inbox --all)
  expect "inbox --all shows injected receipt" contains "$out" "receipt: injected " || ok=1
  expect "inbox --all shows read receipt (s1)" contains "$out" "receipt: read " || ok=1
  expect "await reports injected" contains "$(as "$SENDER" await "$b" --timeout 1)" "$b injected " || ok=1
  reset_recv; return "$ok"
}

s22() { # deliver --format codex / claude: exact hook JSON, properly escaped
  local ok=0 id out ctx
  printf 'quote " backslash \\ tab\there\nsecond line\n' > "$SCRATCH/body22"
  send "$SENDER" worker "json escaping check" --kind ask --body "$SCRATCH/body22"; id=$(last_id)
  out=$(as "$RECV" deliver --format codex); RC=$?
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "one line" eq "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" 1 || ok=1
  expect "starts with the hook shape" re "$out" '^\{"hookSpecificOutput":\{"hookEventName":"UserPromptSubmit","additionalContext":"' || ok=1
  if command -v jq >/dev/null 2>&1; then
    ctx=$(printf '%s' "$out" | jq -r '.hookSpecificOutput.additionalContext'); RC=$?
    expect "valid JSON" eq "$RC" 0 || ok=1
    expect "context has header" contains "$ctx" "id: $id" || ok=1
    expect "quote/backslash/tab round-trip" contains "$ctx" "$(printf 'quote " backslash \\ tab\there')" || ok=1
    expect "newline round-trip" contains "$ctx" "$(printf 'here\nsecond line')" || ok=1
  fi
  send "$SENDER" worker "claude format" --kind fyi --body "$SCRATCH/body22"; id=$(last_id)
  out=$(as "$RECV" deliver --format claude)
  expect "claude format is the same JSON" contains "$out" "\"hookEventName\":\"UserPromptSubmit\",\"additionalContext\":\"from: boss/$SENDER" || ok=1
  expect "silent when empty (codex)" empty "$(as "$RECV" deliver --format codex)" || ok=1
  expect "silent when empty (claude)" empty "$(as "$RECV" deliver --format claude)" || ok=1
  expect "bad format rejected" eq "$(as "$RECV" deliver --format vim >/dev/null 2>&1; echo $?)" 1 || ok=1
  reset_recv; return "$ok"
}

s23() { # deliver needs no tmux server and is fast; hook JSON on stdin is accepted
  local ok=0 id out t0 t1 best=99999 i
  send "$SENDER" worker "no server needed" --kind ask --body "$SCRATCH/body22"; id=$(last_id)
  out=$(cd "$WORKER_DIR" && printf '{"session_id":"x","prompt":"hi"}' | HAIL_SOCKET=/nonexistent/socket TMUX_PANE="$RECV" "$HAIL" deliver --format codex); RC=$?
  expect "rc=0 without a server" eq "$RC" 0 || ok=1
  expect "delivered without a server" contains "$out" "id: $id" || ok=1
  for i in 1 2 3; do
    t0=$(now_ms); (cd "$WORKER_DIR" && HAIL_SOCKET=/nonexistent/socket TMUX_PANE="$RECV" "$HAIL" deliver --format codex </dev/null >/dev/null); t1=$(now_ms)
    (( t1 - t0 < best )) && best=$(( t1 - t0 ))
  done
  # Wall-clock budget for an empty deliver. HAIL_TEST_DELIVER_MS raises it on a
  # loaded machine (the guard is about the script's cost, not the host's load).
  local budget="${HAIL_TEST_DELIVER_MS:-25}"
  expect "empty deliver under ${budget} ms (best of 3: ${best} ms)" le "$best" "$budget" || ok=1
  reset_recv; return "$ok"
}

s24() { # brief: silent when empty; inbox section; sends without receipt; disappears once read
  # nogo: typed and pending like any mail, with no obligation left after the read
  clear_state
  local ok=0 id out
  expect "brief silent when empty" empty "$(as "$RECV" brief)" || ok=1
  send "$SENDER" worker "brief me" --kind nogo --scope herald/x; id=$(last_id)
  out=$(as "$RECV" brief)
  expect "inbox header" contains "$out" "inbox (1 unread)" || ok=1
  expect "envelope-style line" contains "$out" "[hail nogo from:boss id:$id scope:herald/x] brief me" || ok=1
  expect "no sends section on recipient" not_contains "$out" "my sends" || ok=1
  expect "sender brief: fresh send not listed" empty "$(as "$SENDER" brief)" || ok=1
  sed -i.bak "s/^epoch: .*/epoch: $(( $(date +%s) - 200 ))/" "$SEATS/boss/pending/$id" && rm -f "$SEATS/boss/pending/$id.bak"
  out=$(as "$SENDER" brief)
  expect "sends without receipt header" contains "$out" "my sends without receipt (1)" || ok=1
  expect "sends line" contains "$out" "[hail nogo to:worker id:$id] brief me (3m, no receipt)" || ok=1
  as "$RECV" inbox >/dev/null
  expect "sender brief empty after read" empty "$(as "$SENDER" brief)" || ok=1
  expect "recipient brief empty after read" empty "$(as "$RECV" brief)" || ok=1
  expect "brief needs no server" eq "$(HAIL_SOCKET=/nonexistent/socket as "$RECV" brief >/dev/null 2>&1; echo $?)" 0 || ok=1
  reset_recv; return "$ok"
}

s25() { # hold -> release by issuer / refused by another; block; envelope carries re: and scope:
  clear_state
  local ok=0 h b line out
  send "$SENDER" worker "HOLD landing until gate is green" --kind hold --scope murail-ke7is; h=$(last_id)
  expect "hold rc=0" eq "$RC" 0 || ok=1
  expect "hold file" exists "$STATE/holds/$h" || ok=1
  expect "hold issuer" grep -qx "issuer: boss" "$STATE/holds/$h" || ok=1
  line=$(envelope_line "id:$h")
  expect "scope in envelope" contains "$line" "id:$h scope:murail-ke7is until:" || ok=1
  expect "hold typed in full, no hint" not_contains "$line" "hail inbox" || ok=1
  send "$SENDER" worker "BLOCK: anchor dirty" --kind block; b=$(last_id)
  expect "block file" exists "$STATE/holds/$b" || ok=1
  out=$(as "$RECV" brief)
  expect "brief lists holds (2)" contains "$out" "holds / blocks on me (2)" || ok=1
  expect "brief hold line" contains "$out" "[hail hold from:boss to:worker id:$h scope:murail-ke7is] HOLD landing" || ok=1
  expect "brief block line" contains "$out" "[hail block from:boss to:worker id:$b]" || ok=1
  reset_sender
  send "$SENDER" worker "lifted" --kind release
  expect "release without --re refused" eq "$RC" 1 || ok=1
  send "$SENDER" worker "lifted" --kind release --re "$h"
  expect "release by issuer rc=0" eq "$RC" 0 || ok=1
  expect "re: in envelope" contains "$(envelope_line "id:$(last_id)")" " re:$h]" || ok=1
  expect "hold removed" missing "$STATE/holds/$h" || ok=1
  expect "block survives" exists "$STATE/holds/$b" || ok=1
  send "$SENDER" worker "unblocked" --kind release --re "$b"
  expect "block released" missing "$STATE/holds/$b" || ok=1
  expect "no holds section" not_contains "$(as "$RECV" brief)" "holds" || ok=1
  # Lapse: --for sets until:; past it, the hold is gone for every reader, the
  # issuer's brief says so once, and release still finds it.
  send "$SENDER" worker "HOLD briefly" --kind hold --for 30m; h=$(last_id)
  expect "--for in the record" grep -q "^expires: " "$STATE/holds/$h" || ok=1
  send "$SENDER" worker "too long" --kind hold --for 8d
  expect "--for over 7d refused" eq "$RC" 1 || ok=1
  sed -i.bak "s/^expires: .*/expires: 2020-01-01T00:00:00Z/" "$STATE/holds/$h" && rm -f "$STATE/holds/$h.bak"
  expect "lapsed hold not in recipient brief" not_contains "$(as "$RECV" brief)" "$h" || ok=1
  out=$(as "$SENDER" brief)
  expect "issuer told once" contains "$out" "[hail hold to:worker id:$h] HOLD briefly (lapsed 2020-01-01T00:00:00Z)" || ok=1
  expect "only once" not_contains "$(as "$SENDER" brief)" "$h" || ok=1
  send "$SENDER" worker "lifted late" --kind release --re "$h"
  expect "release of a lapsed hold rc=0" eq "$RC" 0 || ok=1
  expect "says it had lapsed" contains "$ERR" "had already lapsed" || ok=1
  expect "retired record gone" missing "$STATE/holds/lapsed/$h" || ok=1
  reset_recv; return "$ok"
}

s26() { # go -> done closes exactly it; wrong re fails; obligation survives a read ("compaction")
  clear_state
  local ok=0 g r out
  send "$SENDER" worker "GO commit on base abc123" --kind go --scope commit; g=$(last_id)
  send "$SENDER" worker "convert at the receipt" --kind ruling; r=$(last_id)
  expect "obligation files" exists "$SEATS/worker/owed/$g" || ok=1
  as "$RECV" inbox >/dev/null          # the envelope is read; the body leaves context at compaction
  out=$(as "$RECV" brief)
  expect "obligations listed after read" contains "$out" "open obligations on me (2)" || ok=1
  expect "go line with scope" contains "$out" "[hail go from:boss id:$g scope:commit] GO commit on base abc123 — hail boss done --re $g" || ok=1
  expect "no inbox section" not_contains "$out" "inbox (" || ok=1
  send "$RECV" boss "committed" --kind done --re nope-0000
  expect "done with wrong re fails" eq "$RC" 1 || ok=1
  expect "wrong re message" contains "$ERR" "no open obligation nope-0000 on worker" || ok=1
  send "$RECV" boss "committed" --kind done
  expect "done without --re fails" eq "$RC" 1 || ok=1
  send "$SENDER" worker "closing your go" --kind done --re "$g"
  expect "done by the wrong party fails" eq "$RC" 1 || ok=1
  send "$RECV" boss "committed" --kind done --re "$g" --scope commit
  expect "done rc=0" eq "$RC" 0 || ok=1
  expect "done envelope has re and scope" contains "$(pane_text "$SENDER")" "id:$(last_id) re:$g scope:commit] committed" || ok=1
  expect "go obligation closed" missing "$SEATS/worker/owed/$g" || ok=1
  expect "ruling obligation open" exists "$SEATS/worker/owed/$r" || ok=1
  expect "brief lists the remaining one" contains "$(as "$RECV" brief)" "open obligations on me (1)" || ok=1
  reset_sender; reset_recv; return "$ok"
}

s27() { # headline over the cap refused for a control kind, folded for others; missing kind refused
  local ok=0 text
  text="STOP: red gate on master, do not land anything until the fan-in gate is green again; this sentence is padded to be long enough to exceed the envelope budget by a comfortable margin ok"
  send "$SENDER" worker "$text" --kind stop
  expect "rc=2" eq "$RC" 2 || ok=1
  expect "message names the cap" contains "$ERR" "headline is $(chars "$text") characters; the cap is 160" || ok=1
  expect "nothing typed" not_contains "$(pane_text "$RECV")" "STOP: red gate" || ok=1
  send "$SENDER" worker "$LONG_ASK" --kind ask
  expect "ask over cap folds, rc=0" eq "$RC" 0 || ok=1
  expect "fold is announced" contains "$ERR" "headline folded to" || ok=1
  expect "folded headline typed" contains "$(pane_text "$RECV")" "Ruling on herald-ke7is: convert at the receipt, not the producer" || ok=1
  expect "folded headline ends with an ellipsis" contains "$(pane_text "$RECV")" " …" || ok=1
  expect "full text in the body" contains "$(cat "$(msgfile "$(last_id)")")" "coordinator should not read panes for replies" || ok=1
  as "$RECV" inbox >/dev/null   # drain the folded message so later scenarios start clean
  send "$SENDER" worker "no kind given"
  expect "missing kind rc=1" eq "$RC" 1 || ok=1
  expect "missing kind names the kinds" contains "$ERR" "(ruling go nogo ask fyi done stop hold block release announce)" || ok=1
  reset_sender; reset_recv; return "$ok"
}

s28() { # a directory with two agents: each is a sub-seat; the bare seat is refused
  local ok=0 dir="$SCRATCH/shared" p1 p2 id out
  mkdir -p "$dir"; echo shared > "$dir/.hail-seat"
  "${T[@]}" new-window -d -t t -c "$dir" cat
  "${T[@]}" split-window -d -t t:1 -c "$dir" cat
  sleep 0.3
  p1=$("${T[@]}" list-panes -t t:1 -F '#{pane_id}' | sed -n 1p); p2=$("${T[@]}" list-panes -t t:1 -F '#{pane_id}' | sed -n 2p)
  send "$SENDER" shared "to both?" --kind fyi
  expect "bare shared seat rc=3 (got $RC)" eq "$RC" 3 || ok=1
  expect "names the sub-seats" contains "$ERR" "address one: shared@$p1 shared@$p2" || ok=1
  send "$SENDER" "shared@$p2" "to the second" --kind ask --body "for p2 only"; id=$(last_id)
  expect "sub-seat send rc=0 ($ERR)" eq "$RC" 0 || ok=1
  expect "in the sub-seat's inbox" exists "$(unread_file "$id" "shared@$p2")" || ok=1
  expect "typed into p2" contains "$("${T[@]}" capture-pane -p -t "$p2")" "id:$id" || ok=1
  expect "not into p1" not_contains "$("${T[@]}" capture-pane -p -t "$p1")" "id:$id" || ok=1
  send "$SENDER" "$p1" "by pane id" --kind fyi
  expect "pane id resolves to its sub-seat" exists "$(unread_file "$(last_id)" "shared@$p1")" || ok=1
  out=$(cd "$dir" && TMUX_PANE="$p1" "$HAIL" deliver)
  expect "p1's hook does not take p2's mail" not_contains "$out" "for p2 only" || ok=1
  out=$(cd "$dir" && TMUX_PANE="$p2" "$HAIL" deliver)
  expect "p2's hook delivers its mail" contains "$out" "for p2 only" || ok=1
  out=$(cd "$dir" && TMUX_PANE="$p2" "$HAIL" send boss fyi 'from p2' 2>&1)
  expect "a sub-seat signs as itself" grep -qxF "from: shared@$p2" "$(msgfile "$(printf '%s\n' "$out" | sed -n 's/^id=//p')")" || ok=1
  "${T[@]}" kill-window -t t:1
  reset_sender; reset_recv; return "$ok"
}

s29() { # bare-target send form
  local ok=0 id
  OUT=$(as "$SENDER" worker "bare form works" --kind ask 2>"$SCRATCH/err"); RC=$?; ERR=$(cat "$SCRATCH/err")
  id=$(last_id)
  expect "rc=0 ($ERR)" eq "$RC" 0 || ok=1
  expect "delivered" exists "$(unread_file "$id" worker)" || ok=1
  expect "envelope" contains "$(envelope_line "id:$id")" "[hail kind:ask from:boss/$SENDER" || ok=1
  expect "help documents the form" contains "$("$HAIL" --help)" "hail <seat> <kind> '<headline>'" || ok=1
  expect "single unknown word is an error" contains "$(as "$SENDER" bogus 2>&1)" "kind is missing" || ok=1
  OUT=$(as "$SENDER" worker ask 'new form, headline argument' 2>"$SCRATCH/err"); RC=$?
  expect "new form rc=0" eq "$RC" 0 || ok=1
  expect "new form typed" contains "$(envelope_line "id:$(last_id)")" "] new form, headline argument" || ok=1
  reset_recv; return "$ok"
}

s30() { # send submits; --no-submit types without Enter; keys after a read submits
  local ok=0 id
  send "$SENDER" worker "submitted for you" --kind ask; id=$(last_id)
  sleep 0.2
  expect "Enter pressed: cat echoed the line (2 copies)" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  send "$SENDER" worker "not submitted" --kind ask --no-submit; id=$(last_id)
  sleep 0.2
  expect "no Enter: one copy" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 1 || ok=1
  expect "keys needs a read first" contains "$(as "$SENDER" keys worker Enter 2>&1)" "hail read" || ok=1
  as "$SENDER" read worker 5 >/dev/null
  as "$SENDER" keys worker Enter; RC=$?
  expect "keys after a read rc=0" eq "$RC" 0 || ok=1
  sleep 0.2
  expect "now two copies" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  reset_recv; return "$ok"
}

s31() { # guard: permission dialog (exit 4), --force
  local ok=0
  recv_showing "Bash(rm -rf build)" "Do you want to proceed?" "  1. Yes" "  2. Yes, and don't ask again" "  3. No" "Esc to cancel"
  send "$SENDER" worker "would approve rm" --kind ask
  expect "dialog rc=4" eq "$RC" 4 || ok=1
  expect "dialog message" contains "$ERR" "shows a permission/approval dialog" || ok=1
  expect "nothing typed" not_contains "$(pane_text "$RECV")" "would approve rm" || ok=1
  send "$SENDER" worker "forced past dialog" --kind ask --force
  expect "--force rc=0" eq "$RC" 0 || ok=1
  # Whatever else the composer shows (a draft, ghost text, an agent panel) is
  # not the sender's problem: the envelope is typed after it.
  recv_showing "some earlier output" "> half a thought the user has not sent"
  send "$SENDER" worker "appended after a draft" --kind ask
  expect "draft does not block rc=0 ($ERR)" eq "$RC" 0 || ok=1
  reset_recv; return "$ok"
}

s32() { # read N returns N lines, the last ones
  local ok=0 out
  recv_showing l1 l2 l3 l4 l5 l6 l7 l8 l9 l10
  out=$(as "$SENDER" read worker 5)
  expect "5 lines (got $(printf '%s\n' "$out" | wc -l | tr -d ' '))" eq "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" 5 || ok=1
  expect "the last ones" eq "$(printf '%s\n' "$out" | head -1)" l6 || ok=1
  out=$(as "$SENDER" read worker 200)
  expect "more than the screen holds is fine" le "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" 200 || ok=1
  reset_recv; return "$ok"
}

s33() { # no agent in the seat: written to the inbox, not typed, exit 5, do not resend
  local ok=0 id
  OUT=$(cd "$BOSS_DIR" && HAIL_AGENT_COMMANDS=claude TMUX_PANE="$SENDER" "$HAIL" worker ask 'nobody home' 2>"$SCRATCH/err" </dev/null); RC=$?; ERR=$(cat "$SCRATCH/err")
  id=$(last_id)
  expect "rc=5 (got $RC)" eq "$RC" 5 || ok=1
  expect "id printed" re "$id" '^[0-9]{4}T[0-9]{6}-[0-9a-f]{4}$' || ok=1
  expect "in the inbox" exists "$(unread_file "$id" worker)" || ok=1
  expect "says do not resend" contains "$ERR" "do not resend" || ok=1
  expect "not typed" not_contains "$(pane_text "$RECV")" "id:$id" || ok=1
  as "$RECV" inbox >/dev/null
  return "$ok"
}

s34() { # no --body: envelope complete (no hint), file unread, deliver repeats the envelope line and receipts; --body: hint + delivered once
  local ok=0 a b line out
  send "$SENDER" worker "headline is the whole message" --kind ask; a=$(last_id)
  line=$(envelope_line "id:$a")
  expect "no fetch hint" not_contains "$line" "hail inbox" || ok=1
  expect "envelope ends with the headline" re "$line" '\] headline is the whole message$' || ok=1
  expect "file written, unread" exists "$(unread_file "$a" worker)" || ok=1
  expect "sent -> delivered" eq "$(as "$SENDER" sent "$a")" delivered || ok=1
  expect "obligation recorded" exists "$SEATS/worker/owed/$a" || ok=1
  out=$(as "$RECV" deliver --format codex); RC=$?
  expect "deliver repeats the envelope line" contains "$out" "$line" || ok=1
  expect "deliver rc=0" eq "$RC" 0 || ok=1
  expect "receipt injected" exists "$SEATS/worker/cur/$a.injected.md" || ok=1
  expect "sent -> injected" re "$(as "$SENDER" sent "$a")" '^injected [0-9]{4}-.*Z$' || ok=1
  send "$SENDER" worker "manual path" --kind fyi; b=$(last_id)
  out=$(as "$RECV" inbox)
  expect "inbox still prints it" contains "$out" "id: $b" || ok=1
  expect "inbox marks it read" re "$(as "$SENDER" sent "$b")" '^read [0-9]{4}-.*Z$' || ok=1
  printf 'the long part\n' > "$SCRATCH/body34"
  send "$SENDER" worker "with a body" --kind ask --body "$SCRATCH/body34"; b=$(last_id)
  expect "hint present with --body" contains "$(envelope_line "id:$b")" "] with a body — hail inbox" || ok=1
  out=$(as "$RECV" deliver)
  expect "body delivered" contains "$out" "the long part" || ok=1
  expect "delivered once" eq "$(printf '%s\n' "$out" | grep -c "id: $b")" 1 || ok=1
  expect "not delivered again" empty "$(as "$RECV" deliver)" || ok=1
  reset_recv; return "$ok"
}

s50() { # quiet fyi: nothing typed once the recipient's hook has run; it arrives with the next prompt
  local ok=0 id out
  as "$RECV" deliver --format claude >"$SCRATCH/out50"   # the worker's prompt hook has run
  send "$SENDER" worker "quiet progress" --kind fyi; id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "says it is quiet" contains "$ERR" "quiet: arrives with worker's next prompt" || ok=1
  sleep 0.3
  expect "nothing typed into the pane" empty "$(envelope_line "id:$id")" || ok=1
  expect "no pending record" missing "$SEATS/boss/pending/$id" || ok=1
  out=$(as "$RECV" deliver)
  expect "next prompt carries it" contains "$out" "] quiet progress" || ok=1
  rm -f "$SEATS/worker/hooked"    # hooks never ran: typed as before
  as "$RECV" deliver >/dev/null   # a hand-run deliver does not count as a hook (hail-zb5)
  expect "hand-run deliver leaves no mark" missing "$SEATS/worker/hooked" || ok=1
  send "$SENDER" worker "typed progress" --kind fyi; id=$(last_id)
  expect "typed when hooks have not run" contains "$(envelope_line "id:$id")" "] typed progress" || ok=1
  reset_recv; return "$ok"
}

scenario 1  "send from inside the pane: ruling, --body -, submitted" s1
scenario 2  "hail never runs bd: --bead ignored with a notice" s2
scenario 3  "sent before read -> delivered" s3
scenario 4  "inbox --peek leaves no receipt" s4
scenario 5  "inbox writes receipt; sent -> read; --all" s5
scenario 6  "sent unknown id -> unknown" s6
scenario 7  "--kind stop typed inline in full; receipt pre-written as inline" s7
scenario 9  "--body file" s9
scenario 10 "--kind bogus rejected" s10
scenario 11 "a pane id target resolves to its seat; from:/reply:" s11
scenario 14 "version resolve id list doctor help" s14
scenario 15 "the seat is the directory; no seat exits 3; HAIL_SEAT only where none" s15
scenario 16 "await success" s16
scenario 17 "await timeout (no tmux server needed)" s17
scenario 18 "await --any" s18
scenario 19 "aliases message / msg" s19
scenario 20 "tmux-bridge symlink and TMUX_BRIDGE_SOCKET fallback" s20
scenario 21 "deliver: plain text, injected receipt, silent when empty, inbox --all" s21
scenario 22 "deliver --format codex / claude: hook JSON, escaping" s22
scenario 23 "deliver without a tmux server, fast, stdin accepted" s23
scenario 24 "brief: silent when empty, inbox, sends without receipt" s24
scenario 25 "hold/block -> release; re:/scope:" s25
scenario 26 "go/ruling -> done closes exactly one; wrong re fails; survives a read" s26
scenario 27 "headline over cap refused (control kind too); missing kind refused" s27
scenario 28 "shared directory: sub-seats, bare seat refused, hooks keep mail apart" s28
scenario 29 "bare-target send form" s29
scenario 30 "send submits; --no-submit does not; keys after a read" s30
scenario 31 "guard: permission dialog, --force" s31
scenario 32 "read <target> N returns exactly N lines" s32
scenario 33 "no agent pane: inbox only, exit 5, do not resend" s33
s36() { # show <id>
  local ok=0 id
  send "$SENDER" worker "shown by id" --kind ask --body "the body to show"
  id=$(last_id)
  expect "show prints the body" contains "$("$HAIL" show "$id")" "the body to show" || ok=1
  expect "show claims nothing" exists "$(unread_file "$id" worker)" || ok=1
  expect "show unknown id fails" contains "$("$HAIL" show nope-0000 2>&1)" "no message with id nope-0000" || ok=1
  expect "a typo'd verb points at help" contains "$(as "$SENDER" shw "$id" 2>&1)" "hail help" || ok=1
  reset_recv; return "$ok"
}

s35() { # --body literal text; over-cap headline folds into the body
  local ok=0
  send "$SENDER" worker "literal body" --kind ask --body "detail line one, not a file"
  local id; id=$(last_id)
  expect "literal body stored" contains "$(cat "$(msgfile "$id")")" "detail line one, not a file" || ok=1
  expect "hint present" contains "$(envelope_line "$id")" "— hail inbox" || ok=1
  local long; long=$(printf 'x%.0s' $(seq 1 200))
  send "$SENDER" worker "$long" --kind ask; id=$(last_id)
  expect "over cap folds, rc=0" eq "$RC" 0 || ok=1
  expect "fold announced" contains "$ERR" "headline folded to" || ok=1
  expect "folded body holds the full text" contains "$(cat "$(msgfile "$id")")" "$long" || ok=1
  expect "folded envelope carries a fetch hint" contains "$(envelope_line "$id")" "— hail inbox" || ok=1
  as "$RECV" inbox >/dev/null
  reset_sender; reset_recv; return "$ok"
}

scenario 34 "no --body: complete envelope, deliver repeats it with receipt; --body delivered once" s34
scenario 35 "--body literal text; over-cap refusal names --body" s35
scenario 36 "show <id>: one body by id, no receipt; missing id and missing --kind name it" s36

s37() { # a stale TMUX_PANE is ignored: a process reparented to pid 1 in the worker's directory is the worker
  local ok=0 out
  orphan() {
    rm -f "$SCRATCH/orphan.out"
    (cd "$1" && TMUX_PANE="$SENDER" perl -e 'use POSIX; if (fork) { exit 0 } POSIX::setsid(); if (fork) { exit 0 } sleep 0.3; exec(@ARGV)' "$HAIL" whoami >"$SCRATCH/orphan.out" 2>&1)
    for _ in 1 2 3 4 5 6 7 8 9 10; do [[ -s "$SCRATCH/orphan.out" ]] && break; sleep 0.2; done
    cat "$SCRATCH/orphan.out"
  }
  out=$(orphan "$WORKER_DIR")
  expect "the directory's seat, not the stale TMUX_PANE's" contains "$out" "seat: worker" || ok=1
  return "$ok"
}

scenario 37 "a stale TMUX_PANE is ignored: the directory names the seat" s37

s38() { # a command under a process named as Codex's app-server, started from the boss pane, in the worker's directory
  local ok=0 out="$SCRATCH/s38.out"
  rm -f "$out"
  cat > "$SCRATCH/s38.sh" <<EOS
(exec -a 'codex app-server' bash -c 'cd "\$1" && "\$2" whoami >"\$3.tmp" 2>&1; mv "\$3.tmp" "\$3"' _ '$WORKER_DIR' '$HAIL' '$out') &
EOS
  "${T[@]}" send-keys -t "$SENDER" -l -- ". '$SCRATCH/s38.sh'; clear"
  "${T[@]}" send-keys -t "$SENDER" Enter
  expect "daemon's command finished" wait_for_file "$out" || ok=1
  expect "signs as the worker, not the pane that started the daemon" contains "$(cat "$out" 2>/dev/null)" "seat: worker" || ok=1
  reset_sender; return "$ok"
}

scenario 38 "a command under Codex's app-server signs as its directory's seat" s38

s39() { # a pane in copy mode: keys would run mode commands, not reach the composer
  local ok=0 id
  "${T[@]}" set-option -g mode-keys vi
  "${T[@]}" copy-mode -t "$RECV"
  send "$SENDER" worker "scrolled back: check this" --kind ask; id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "the pane left copy mode" eq "$("${T[@]}" display-message -t "$RECV" -p '#{pane_in_mode}')" 0 || ok=1
  expect "envelope typed into the pane" contains "$(pane_text "$RECV")" "id:$id" || ok=1
  "${T[@]}" copy-mode -t "$RECV"
  as "$SENDER" read "$RECV" 5 >/dev/null 2>&1
  as "$SENDER" keys "$RECV" Enter >/dev/null 2>&1
  expect "keys leaves copy mode too" eq "$("${T[@]}" display-message -t "$RECV" -p '#{pane_in_mode}')" 0 || ok=1
  "${T[@]}" set-option -gu mode-keys
  reset_recv; return "$ok"
}

scenario 39 "a send to a pane in copy mode leaves the mode before typing" s39

s40() { # guard: Codex's "Approved" is not a dialog; its approval dialog is
  local ok=0
  recv_showing "✔ Approved command: just land" "• Ran just land" "  └ ok" "⚠ 4 warnings · f2 to view" "› "
  send "$SENDER" worker "idle codex pane" --kind ask
  expect "idle pane after Approved: rc=0" eq "$RC" 0 || ok=1
  recv_showing "Would you like to run the following command?" "  \$ rm -rf build" "› 1. Yes, proceed (y)" "  2. Yes, and don't ask again for this command in this session (a)" "  3. No, and tell Codex what to do differently (esc)" "Press enter to confirm or esc to cancel"
  send "$SENDER" worker "would approve rm" --kind ask
  expect "codex dialog rc=4" eq "$RC" 4 || ok=1
  expect "nothing typed" not_contains "$(pane_text "$RECV")" "would approve rm" || ok=1
  reset_recv; return "$ok"
}

scenario 40 "guard: Codex's Approved status is not a dialog; its approval dialog is" s40

s41() { # heredoc send: headline line, body with shell syntax arrives byte-identical
  local ok=0 id f
  OUT=$(cd "$BOSS_DIR" && TMUX_PANE="$SENDER" "$HAIL" worker ask 2>"$SCRATCH/err" <<'EOF'
Review `auth` and $(whoami); it's == fine
Body with `backticks`, $(rm -rf /nope), 'quotes', "doubles" and \backslash
second line
EOF
); RC=$?; id=$(last_id); f=$(msgfile "$id")
  expect "rc=0 ($(cat "$SCRATCH/err"))" eq "$RC" 0 || ok=1
  expect "headline verbatim" grep -qxF 'ask: Review `auth` and $(whoami); it'"'"'s == fine' "$f" || ok=1
  expect "body verbatim" grep -qxF 'Body with `backticks`, $(rm -rf /nope), '"'"'quotes'"'"', "doubles" and \backslash' "$f" || ok=1
  expect "hint present" contains "$(envelope_line "id:$id")" "— hail inbox" || ok=1
  as "$RECV" inbox >/dev/null
  reset_recv; return "$ok"
}

s42() { # a headline argument never reads stdin, even an open pipe that never ends
  local ok=0 start end
  mkfifo "$SCRATCH/fifo42"
  sleep 30 > "$SCRATCH/fifo42" &
  local holder=$!
  start=$(date +%s)
  (cd "$BOSS_DIR" && TMUX_PANE="$SENDER" "$HAIL" worker fyi 'stdin stays shut' <"$SCRATCH/fifo42" >/dev/null 2>&1); RC=$?
  end=$(date +%s)
  kill "$holder" 2>/dev/null
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "returned promptly" le $(( end - start )) 3 || ok=1
  (cd "$BOSS_DIR" && TMUX_PANE="$SENDER" "$HAIL" worker fyi </dev/null >/dev/null 2>"$SCRATCH/err"); RC=$?
  expect "no headline anywhere: rc=1" eq "$RC" 1 || ok=1
  expect "says where a headline goes" contains "$(cat "$SCRATCH/err")" "no headline" || ok=1
  as "$RECV" inbox >/dev/null
  reset_recv; return "$ok"
}

s43() { # setup in a scratch HOME: installs both harnesses' hooks, replaces 0.3 lines, idempotent
  local ok=0 h="$SCRATCH/home43"
  mkdir -p "$h/.claude" "$h/.codex"
  printf '{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"[ -n \\"$TMUX_PANE\\" ] || exit 0; hail deliver --format claude"}]}]}}\n' > "$h/.claude/settings.json"
  printf '# keep me\nmodel = "x"\n' > "$h/.codex/config.toml"
  HOME="$h" "$HAIL" setup --check >/dev/null 2>&1; RC=$?
  expect "--check reports drift: rc=1" eq "$RC" 1 || ok=1
  HOME="$h" "$HAIL" setup --yes >/dev/null 2>&1; RC=$?
  expect "setup rc=0" eq "$RC" 0 || ok=1
  expect "claude deliver hook" contains "$(cat "$h/.claude/settings.json")" '"hail deliver --format claude"' || ok=1
  expect "0.3 line gone" not_contains "$(cat "$h/.claude/settings.json")" 'TMUX_PANE' || ok=1
  expect "codex hooks added" contains "$(cat "$h/.codex/config.toml")" 'command = "hail brief --hook"' || ok=1
  expect "codex comment kept" contains "$(cat "$h/.codex/config.toml")" '# keep me' || ok=1
  HOME="$h" "$HAIL" setup --check >/dev/null 2>&1; RC=$?
  expect "then current: rc=0" eq "$RC" 0 || ok=1
  return "$ok"
}

s44() { # migrate a 0.3 tree: unread lands in the seat, receipts keep their kind and time, hooks wait for it
  local ok=0 st="$SCRATCH/state44" out
  mkdir -p "$st/hail/inbox/worker" "$st/hail/obligations/worker" "$st/hail/identity"
  printf 'from: boss/%%0\nreply: %%0\nkind: ask\nid: 0101T000000-aaaa\ntime: 2026-01-01T00:00:00Z\nask: old unread\n\nold body\n' > "$st/hail/inbox/worker/0101T000000-aaaa.md"
  printf 'from: boss/%%0\nreply: %%0\nkind: fyi\nid: 0101T000001-bbbb\ntime: 2026-01-01T00:00:01Z\nask: old read\n\nx\n' > "$st/hail/inbox/worker/0101T000001-bbbb.md"
  echo "injected 2026-01-02T03:04:05Z" > "$st/hail/inbox/worker/0101T000001-bbbb.read"
  printf 'id: 0101T000000-aaaa\nkind: ask\nissuer: boss\nto: worker\n' > "$st/hail/obligations/worker/0101T000000-aaaa"
  out=$(cd "$WORKER_DIR" && XDG_STATE_HOME="$st" "$HAIL" deliver)
  expect "hook silent before migrate" empty "$out" || ok=1
  out=$(cd "$WORKER_DIR" && XDG_STATE_HOME="$st" "$HAIL" inbox 2>&1); RC=$?
  expect "other verbs ask for migrate" contains "$out" "hail migrate" || ok=1
  XDG_STATE_HOME="$st" "$HAIL" migrate >/dev/null 2>&1; RC=$?
  expect "migrate rc=0" eq "$RC" 0 || ok=1
  expect "receipt kept" eq "$(cd "$BOSS_DIR" && XDG_STATE_HOME="$st" "$HAIL" sent 0101T000001-bbbb)" "injected 2026-01-02T03:04:05Z" || ok=1
  expect "unread still unread" eq "$(cd "$BOSS_DIR" && XDG_STATE_HOME="$st" "$HAIL" sent 0101T000000-aaaa)" "delivered" || ok=1
  out=$(cd "$WORKER_DIR" && XDG_STATE_HOME="$st" "$HAIL" brief)
  expect "brief lists the unread and the obligation" contains "$out" "open obligations on me (1)" || ok=1
  expect "old tree archived" exists "$st/hail/archive/0.3/inbox" || ok=1
  XDG_STATE_HOME="$st" "$HAIL" migrate --revert >/dev/null 2>&1; RC=$?
  expect "revert rc=0" eq "$RC" 0 || ok=1
  expect "revert restores 0.3 files" exists "$st/hail/inbox/worker/0101T000000-aaaa.md" || ok=1
  expect "revert writes 0.3 receipts" eq "$(cat "$st/hail/inbox/worker/0101T000001-bbbb.read")" "injected 2026-01-02T03:04:05Z" || ok=1
  return "$ok"
}

scenario 41 "heredoc send: shell syntax in headline and body arrives byte-identical" s41
scenario 42 "a headline argument never reads stdin; no headline anywhere is refused" s42
scenario 43 "setup installs hooks for both harnesses, replaces 0.3 lines, idempotent" s43
scenario 44 "migrate imports 0.3 state; hooks wait; revert restores it" s44

s49() { # sub-agents: seat/name reaches the parent, marked for: in the envelope; seat/%N is the pane, checked
  local ok=0 id f
  send "$SENDER" worker/scout ask "for the sub-agent"; id=$(last_id)
  expect "rc=0 ($ERR)" eq "$RC" 0 || ok=1
  f=$(unread_file "$id" worker)
  expect "in the parent's inbox" exists "$f" || ok=1
  expect "for: header" grep -qxF "for: scout" "$f" || ok=1
  expect "envelope typed into the parent with for:" contains "$(envelope_line "id:$id")" "id:$id for:scout]" || ok=1
  OUT=$(as "$RECV" boss fyi "answer from the sub-agent" --as scout 2>"$SCRATCH/err"); id=$(last_id)
  expect "--as signs seat/name" grep -qxF "from: worker/scout" "$(msgfile "$id")" || ok=1
  expect "--as replies to seat/name" grep -qxF "reply: worker/scout" "$(msgfile "$id")" || ok=1
  send "$SENDER" "worker/$RECV" fyi "pasted from: value"; id=$(last_id)
  expect "seat/%N rc=0 ($ERR)" eq "$RC" 0 || ok=1
  expect "seat/%N goes to that pane's seat" exists "$(unread_file "$id" worker)" || ok=1
  send "$SENDER" "boss/$RECV" fyi "wrong seat for the pane"
  expect "seat/%N in another seat: rc=3 (got $RC)" eq "$RC" 3 || ok=1
  expect "names the pane's seat" contains "$ERR" "is in seat worker, not boss" || ok=1
  as "$RECV" inbox >/dev/null; as "$SENDER" inbox >/dev/null
  reset_recv; reset_sender; return "$ok"
}

s45() { # real agent detection (no HAIL_AGENT_COMMANDS): an agent as a pane's root, as the root shell's child, and none
  local ok=0 bin="$SCRATCH/agents" d1="$SCRATCH/a-root" d2="$SCRATCH/a-child" d3="$SCRATCH/a-none" p
  mkdir -p "$bin" "$d1" "$d2" "$d3"
  ln -s /bin/cat "$bin/claude"   # a copied system binary fails its signature check on macOS
  echo a-root > "$d1/.hail-seat"; echo a-child > "$d2/.hail-seat"; echo a-none > "$d3/.hail-seat"
  "${T[@]}" new-window -d -t t:5 -c "$d1" "$bin/claude"
  # The root shell starts the agent in the background, then becomes a tool
  # (sleep): the pane's foreground is not the agent, its root's child is.
  mkdir -p "$bin/long"; ln -s /bin/sleep "$bin/long/claude"
  "${T[@]}" split-window -d -t t:5 -c "$d2" "bash --norc -c '\"$bin/long/claude\" 300 & exec sleep 300'"
  "${T[@]}" split-window -d -t t:5 -c "$d3" "sleep 300"
  sleep 0.5
  local seats; seats=$(cd "$BOSS_DIR" && env -u HAIL_AGENT_COMMANDS "$HAIL" seats)
  expect "root agent seen ($seats)" re "$seats" 'a-root +%[0-9]+:claude' || ok=1
  expect "child-of-shell agent seen" re "$seats" 'a-child +%[0-9]+:claude' || ok=1
  expect "no agent in a sleep pane" re "$seats" 'a-none +- ' || ok=1
  (cd "$BOSS_DIR" && env -u HAIL_AGENT_COMMANDS TMUX_PANE="$SENDER" "$HAIL" a-root fyi 'to a real agent' >/dev/null 2>&1); RC=$?
  expect "wakes the root agent: rc=0 (got $RC)" eq "$RC" 0 || ok=1
  (cd "$BOSS_DIR" && env -u HAIL_AGENT_COMMANDS TMUX_PANE="$SENDER" "$HAIL" a-none fyi 'nobody' >/dev/null 2>&1); RC=$?
  expect "no agent: rc=5 (got $RC)" eq "$RC" 5 || ok=1
  "${T[@]}" kill-window -t t:5
  return "$ok"
}

s46() { # sharing ends: a sub-seat's mail, obligations and replies stay reachable
  local ok=0 dir="$SCRATCH/shared2" p1 p2 a b out
  mkdir -p "$dir"; echo shared2 > "$dir/.hail-seat"
  "${T[@]}" new-window -d -t t:6 -c "$dir" cat
  "${T[@]}" split-window -d -t t:6 -c "$dir" cat
  sleep 0.3
  p1=$("${T[@]}" list-panes -t t:6 -F '#{pane_id}' | sed -n 1p); p2=$("${T[@]}" list-panes -t t:6 -F '#{pane_id}' | sed -n 2p)
  send "$SENDER" "shared2@$p2" "first, while shared" --kind ask --body "body one"; a=$(last_id)
  "${T[@]}" kill-pane -t "$p1"; sleep 0.2
  send "$SENDER" "shared2@$p2" "second, after sharing ended" --kind ask --body "body two"; b=$(last_id)
  expect "reply to the old sub-seat rc=0 ($ERR)" eq "$RC" 0 || ok=1
  out=$(cd "$dir" && TMUX_PANE="$p2" "$HAIL" brief)
  expect "brief lists both obligations" contains "$out" "open obligations on me (2)" || ok=1
  out=$(cd "$dir" && TMUX_PANE="$p2" "$HAIL" inbox)
  expect "inbox reads the sub-seat after sharing ended" contains "$out" "body two" || ok=1
  expect "and the earlier one" contains "$out" "body one" || ok=1
  (cd "$dir" && TMUX_PANE="$p2" "$HAIL" boss done --re "$a" 'did one' </dev/null >/dev/null 2>&1); RC=$?
  expect "done closes a sub-seat obligation: rc 0 or 5 (got $RC)" re "$RC" '^(0|5)$' || ok=1
  out=$(cd "$dir" && CODEX_THREAD_ID=x TMUX_PANE="$p2" "$HAIL" brief)
  expect "a Codex command never reads a sub-seat" not_contains "$out" "obligations" || ok=1
  "${T[@]}" kill-window -t t:6
  reset_sender; return "$ok"
}

s47() { # a headline argument and a heredoc together: the heredoc is the body
  local ok=0 id
  OUT=$(cd "$BOSS_DIR" && TMUX_PANE="$SENDER" "$HAIL" worker ask 'headline as an argument' 2>/dev/null <<'EOF'
the heredoc body
EOF
); id=$(last_id)
  expect "body kept" grep -qxF "the heredoc body" "$(msgfile "$id")" || ok=1
  expect "hint present" contains "$(envelope_line "id:$id")" "] headline as an argument — hail inbox" || ok=1
  local d
  for d in 0.05 0.5; do
    OUT=$( (sleep "$d"; printf 'piped after %s s\n' "$d") | (cd "$BOSS_DIR" && TMUX_PANE="$SENDER" "$HAIL" worker fyi "headline, body piped late" 2>/dev/null) ); id=$(last_id)
    expect "a body piped $d s late is kept" grep -qxF "piped after $d s" "$(msgfile "$id")" || ok=1
  done
  as "$RECV" inbox >/dev/null
  reset_recv; return "$ok"
}

s48() { # migrating messy real-world state; a backlog is delivered a few bodies per prompt
  local ok=0 st="$SCRATCH/state48" out i m
  mkdir -p "$st/hail/inbox/$RECV" "$st/hail/inbox/%999" "$st/hail/inbox/oldlabel" "$st/hail/inbox/worker" "$st/hail/obligations/$RECV"
  m() { printf 'from: boss/%%0\nreply: %%0\nkind: fyi\nid: %s\ntime: 2026-10-01T00:00:00Z\nask: %s\n\n%s\n' "$1" "$2" "$3"; }
  m 1001T000000-aaa1 "to a live pane" "fresh pane mail" > "$st/hail/inbox/$RECV/1001T000000-aaa1.md"
  m 1001T000000-aaa2 "old pane mail" "stale" > "$st/hail/inbox/$RECV/1001T000000-aaa2.md"; touch -t 202601010000 "$st/hail/inbox/$RECV/1001T000000-aaa2.md"
  m 1001T000000-aaa3 "dead pane" "x" > "$st/hail/inbox/%999/1001T000000-aaa3.md"
  m 1001T000000-aaa4 "old label" "x" > "$st/hail/inbox/oldlabel/1001T000000-aaa4.md"
  for i in 1 2 3 4 5 6 7 8; do m "1001T00000$i-bbb$i" "backlog $i" "backlog body $i" > "$st/hail/inbox/worker/1001T00000$i-bbb$i.md"; done
  printf 'id: x\nkind: ask\n' > "$st/hail/obligations/$RECV/1001T000000-ccc1"
  out=$(XDG_STATE_HOME="$st" "$HAIL" migrate 2>&1); RC=$?
  expect "migrate rc=0" eq "$RC" 0 || ok=1
  expect "reports mail no agent will read" contains "$out" "oldlabel: 1 unread" || ok=1
  expect "recent unread pane mail goes to the pane's seat" exists "$st/hail/seats/worker/new/1001T000000-aaa1.md" || ok=1
  expect "old pane mail is parked" exists "$st/hail/seats/legacy-$RECV/new/1001T000000-aaa2.md" || ok=1
  expect "dead pane parked" exists "$st/hail/seats/legacy-%999/new/1001T000000-aaa3.md" || ok=1
  expect "pane-keyed obligations parked" exists "$st/hail/seats/legacy-$RECV/owed/1001T000000-ccc1" || ok=1
  out=$(cd "$WORKER_DIR" && XDG_STATE_HOME="$st" TMUX_PANE="$RECV" "$HAIL" deliver)
  expect "first prompt: five bodies" eq "$(printf '%s\n' "$out" | grep -c '^ask: ')" 5 || ok=1
  expect "and how many wait" contains "$out" "4 more unread" || ok=1
  out=$(cd "$WORKER_DIR" && XDG_STATE_HOME="$st" TMUX_PANE="$RECV" "$HAIL" deliver)
  expect "next prompt: the rest" eq "$(printf '%s\n' "$out" | grep -c '^ask: ')" 4 || ok=1
  XDG_STATE_HOME="$st" "$HAIL" migrate --revert >/dev/null 2>&1
  expect "revert puts legacy pane mail back under its pane id" exists "$st/hail/inbox/$RECV/1001T000000-aaa2.md" || ok=1
  expect "and pane-keyed obligations" exists "$st/hail/obligations/$RECV/1001T000000-ccc1" || ok=1
  return "$ok"
}

scenario 45 "real agent detection: root, child of the root shell, none" s45
scenario 46 "sharing ends: sub-seat mail and obligations stay reachable; Codex never reads one" s46
scenario 47 "a headline argument with a heredoc or a late pipe: that is the body" s47
scenario 48 "migration of pane keys, old labels and a backlog; deliver caps per prompt" s48
scenario 49 "sub-agents: seat/name via the parent with for:, --as signs; seat/%N is the pane, checked" s49
scenario 50 "quiet fyi once hooks run; typed where they have not" s50

echo "---"
echo "passed $PASS, failed $FAIL"
if (( FAIL > 0 )); then echo "failed scenarios: ${FAILED[*]}"; exit 1; fi
exit 0
