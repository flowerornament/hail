#!/usr/bin/env bash
# hail test harness — runs every scenario in SCENARIOS.md plus the await and
# alias scenarios against a scratch tmux server (-L hailtest). Never touches
# the default tmux server: every tmux call here names the scratch socket and
# hail is pointed at it with HAIL_SOCKET.
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
HAIL="$HERE/../bin/hail"
SOCKNAME=hailtest
T=(tmux -L "$SOCKNAME")

unset TMUX TMUX_PANE HAIL_SOCKET TMUX_BRIDGE_SOCKET

SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/hailtest.XXXXXX")
export XDG_STATE_HOME="$SCRATCH/state"
INBOX="$XDG_STATE_HOME/hail/inbox"

cleanup() {
  "${T[@]}" kill-server 2>/dev/null || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

# --- fake bd shims -----------------------------------------------------------
# bd-fail: present but every comment fails (scenario 2). bd-ok: returns a
# comment id (scenario 12). The failing shim is on PATH for the whole run so
# the real bd (and the user's beads db) is never touched.
mkdir -p "$SCRATCH/bd-fail" "$SCRATCH/bd-ok" "$SCRATCH/nobd" "$SCRATCH/bin"
printf '#!/bin/sh\nexit 1\n' > "$SCRATCH/bd-fail/bd"
printf '#!/bin/sh\necho "{\\"id\\": 7}"\n' > "$SCRATCH/bd-ok/bd"
chmod +x "$SCRATCH/bd-fail/bd" "$SCRATCH/bd-ok/bd"
for tool in tmux fswatch; do
  p=$(command -v "$tool" 2>/dev/null) && ln -s "$p" "$SCRATCH/nobd/$tool"
done
ln -s "$HAIL" "$SCRATCH/bin/tmux-bridge"
ln -s "$HAIL" "$SCRATCH/bin/hail"
BASE_PATH="$PATH"
export PATH="$SCRATCH/bd-fail:$BASE_PATH"
NOBD_PATH="$SCRATCH/nobd:/usr/bin:/bin"

# --- scratch server ----------------------------------------------------------
"${T[@]}" kill-server 2>/dev/null || true
"${T[@]}" -f /dev/null new-session -d -s t -x 300 -y 50 'bash --norc' || { echo "cannot start scratch tmux server"; exit 2; }
"${T[@]}" split-window -t t -d cat
PANES=$("${T[@]}" list-panes -t t -F '#{pane_id}')
SENDER=$(printf '%s\n' "$PANES" | sed -n 1p); RECV=$(printf '%s\n' "$PANES" | sed -n 2p)
export HAIL_SOCKET
HAIL_SOCKET=$("${T[@]}" display-message -p '#{socket_path}')
[[ -S "$HAIL_SOCKET" ]] || { echo "scratch socket missing: $HAIL_SOCKET"; exit 2; }
sleep 0.3

# --- helpers -----------------------------------------------------------------
PASS=0; FAIL=0; FAILED=()
as() { local pane="$1"; shift; TMUX_PANE="$pane" "$HAIL" "$@"; }
pane_text() { "${T[@]}" capture-pane -t "$1" -p -J; }
reset_recv() { "${T[@]}" respawn-pane -k -t "$RECV" cat; sleep 0.2; }
reset_sender() { "${T[@]}" send-keys -t "$SENDER" C-c; sleep 0.1; }
# send FROM TARGET args... : satisfy the read guard, then send. stdout/stderr/rc
# land in $OUT/$ERR/$RC.
OUT=""; ERR=""; RC=0
send() {
  local from="$1" target="$2"; shift 2
  as "$from" read "$target" 5 >/dev/null 2>&1
  OUT=$(as "$from" send "$target" "$@" 2>"$SCRATCH/err"); RC=$?
  ERR=$(cat "$SCRATCH/err")
}
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
as "$SENDER" name "$SENDER" boss
as "$SENDER" name "$RECV" worker

LONG_ASK='Ruling on herald-ke7is: convert at the receipt, not the producer; the fan-in gate hashes checkout contents so scratch state must stay outside every repo tree, and the coordinator should not read panes for replies'

s1() { # from inside the sender pane via send-keys, --kind ruling --body -
  printf 'line one of the body\nline two of the body\n' > "$SCRATCH/body1"
  local ok=0
  # The pane runs the script so TMUX_PANE comes from tmux itself, not from
  # the harness. A short typed line: readline stalls on lines past ~1 KB.
  cat > "$SCRATCH/s1.sh" <<EOS
export HAIL_SOCKET='$HAIL_SOCKET' XDG_STATE_HOME='$XDG_STATE_HOME' PATH='$PATH'
'$HAIL' read '$RECV' 5 >/dev/null
'$HAIL' send worker '$LONG_ASK' --kind ruling --body - <'$SCRATCH/body1' >'$SCRATCH/s1.out' 2>'$SCRATCH/s1.err'
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
  expect "envelope head" contains "$line" "[hail kind:ruling from:boss/$SENDER reply:$SENDER id:$id bead:herald-ke7is]" || ok=1
  expect "fetch hint" contains "$line" "— hail inbox" || ok=1
  expect "ask truncated with …" contains "$line" "…" || ok=1
  expect "envelope exactly 160 chars (got $(chars "$line"))" eq "$(chars "$line")" 160 || ok=1
  local f="$INBOX/worker/$id.md"
  expect "inbox file exists" exists "$f" || ok=1
  expect "file has full ask" grep -qF "ask: $LONG_ASK" "$f" || ok=1
  expect "file has stdin body" grep -qF "line two of the body" "$f" || ok=1
  expect "file header from:" grep -qF "from: boss/$SENDER" "$f" || ok=1
  S1_ID="$id"
  reset_recv
  return "$ok"
}

s2() { # bd present, comment fails
  local ok=0
  send "$SENDER" worker "please look at herald-ke7is again" --kind ask
  local id; id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "one warning line" eq "$(printf '%s\n' "$ERR" | grep -c 'hail: warning: could not post to bead herald-ke7is')" 1 || ok=1
  expect "file says not posted" grep -qF "bead: herald-ke7is (not posted)" "$INBOX/worker/$id.md" || ok=1
  expect "delivered" contains "$(envelope_line "id:$id")" "bead:herald-ke7is]" || ok=1
  reset_recv; return "$ok"
}

s3() { eq "$(as "$SENDER" sent "$S1_ID")" delivered; }

s4() { # inbox --peek
  local ok=0 out
  out=$(as "$RECV" inbox --peek)
  expect "peek prints header" contains "$out" "from: boss/$SENDER" || ok=1
  expect "peek prints body" contains "$out" "line two of the body" || ok=1
  expect "no .read written" missing "$INBOX/worker/$S1_ID.read" || ok=1
  expect "sent still delivered" eq "$(as "$SENDER" sent "$S1_ID")" delivered || ok=1
  return "$ok"
}

s5() { # inbox writes receipts
  local ok=0 out
  out=$(as "$RECV" inbox)
  expect "prints s1" contains "$out" "id: $S1_ID" || ok=1
  expect ".read written" exists "$INBOX/worker/$S1_ID.read" || ok=1
  expect ".read holds UTC time" grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' "$INBOX/worker/$S1_ID.read" || ok=1
  expect "sent -> read <time>" re "$(as "$SENDER" sent "$S1_ID")" '^read [0-9]{4}-.*Z$' || ok=1
  expect "second inbox empty" eq "$(as "$RECV" inbox)" "(inbox empty)" || ok=1
  expect "--all shows it again" contains "$(as "$RECV" inbox --all)" "id: $S1_ID" || ok=1
  return "$ok"
}

s6() { [[ "$(as "$SENDER" sent nope-0000)" == unknown ]]; }

s7() { # --kind stop, 190 chars typed inline
  local ok=0 text line id
  text="STOP: red gate on master, do not land anything until the fan-in gate is green again; this sentence is padded to be long enough to exceed the envelope budget by a comfortable margin ok"
  send "$SENDER" worker "$text" --kind stop
  id=$(last_id)
  line=$(envelope_line "id:$id")
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "full text inline" contains "$line" "] $text" || ok=1
  expect "no …" not_contains "$line" "…" || ok=1
  expect "no fetch hint" not_contains "$line" "hail inbox" || ok=1
  expect "file still written" exists "$INBOX/worker/$id.md" || ok=1
  reset_recv; return "$ok"
}

s8() { # hyphenated words are not beads
  local ok=0 id line
  send "$SENDER" worker "herald-abc.2 and tmux-bridge and read-only are not beads" --kind fyi
  id=$(last_id); line=$(envelope_line "id:$id")
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "no bead: in envelope" not_contains "$line" "bead:" || ok=1
  expect "no warning" empty "$ERR" || ok=1
  reset_recv; return "$ok"
}

s9() { # --bead + --body file
  local ok=0 id line
  printf 'body from a file\n' > "$SCRATCH/body9"
  send "$SENDER" worker "ruling attached" --kind ruling --bead murail-zz9zz --body "$SCRATCH/body9"
  id=$(last_id); line=$(envelope_line "id:$id")
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "body from file" grep -qF "body from a file" "$INBOX/worker/$id.md" || ok=1
  expect "warning on bd failure" contains "$ERR" "could not post to bead murail-zz9zz" || ok=1
  expect "bead: in envelope" contains "$line" "bead:murail-zz9zz]" || ok=1
  reset_recv; return "$ok"
}

s10() { # --kind bogus
  local ok=0
  send "$SENDER" worker "hello" --kind bogus
  expect "rc=1" eq "$RC" 1 || ok=1
  expect "lists kinds" contains "$ERR" "ruling go nogo announce ask fyi stop hold" || ok=1
  return "$ok"
}

s11() { # labeled -> unlabeled pane; inbox keyed on pane id
  local ok=0 id
  "${T[@]}" set-option -p -t "$SENDER" -u @name
  send "$RECV" "$SENDER" "back at you" --kind fyi
  id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "inbox dir keyed on pane id" exists "$INBOX/$SENDER/$id.md" || ok=1
  expect "from: worker/$RECV" grep -qF "from: worker/$RECV" "$INBOX/$SENDER/$id.md" || ok=1
  expect "envelope in sender pane" contains "$(pane_text "$SENDER")" "id:$id" || ok=1
  expect "inbox as sender reads it" contains "$(as "$SENDER" inbox)" "id: $id" || ok=1
  reset_sender
  as "$SENDER" name "$SENDER" boss
  return "$ok"
}

s12() { # fake bd returning a comment id
  local ok=0 id
  PATH="$SCRATCH/bd-ok:$BASE_PATH" send "$SENDER" worker "RULED on murail-ke7is: convert at the receipt" --kind ruling
  id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "stdout bead=... comment=7" contains "$OUT" "bead=murail-ke7is comment=7" || ok=1
  expect "file comment line" grep -qF "bead: murail-ke7is (comment 7)" "$INBOX/worker/$id.md" || ok=1
  expect "file see: line" grep -qF "see: bd show murail-ke7is" "$INBOX/worker/$id.md" || ok=1
  reset_recv; return "$ok"
}

s13() { # no bd on PATH, --kind hold
  local ok=0 id
  PATH="$NOBD_PATH" send "$SENDER" worker "HOLD murail-ke7is until the gate is green" --kind hold
  id=$(last_id)
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "warning" contains "$ERR" "could not post to bead murail-ke7is" || ok=1
  expect "file-only" grep -qF "bead: murail-ke7is (not posted)" "$INBOX/worker/$id.md" || ok=1
  reset_recv; return "$ok"
}

s14() { # version resolve id list doctor help
  local ok=0
  expect "version" eq "$(as "$SENDER" version)" "hail 1.0.0" || ok=1
  expect "resolve worker" eq "$(as "$SENDER" resolve worker)" "$RECV" || ok=1
  expect "id" eq "$(as "$SENDER" id)" "$SENDER" || ok=1
  expect "list shows label" contains "$(as "$SENDER" list)" "worker" || ok=1
  expect "doctor OK" contains "$(as "$SENDER" doctor)" "Status: OK" || ok=1
  expect "help mentions await" contains "$("$HAIL" --help)" "await <id>..." || ok=1
  expect "help has no tmux-bridge text" not_contains "$("$HAIL" --help | grep -v TMUX_BRIDGE_SOCKET)" "tmux-bridge" || ok=1
  return "$ok"
}

s15() { # static checks
  local ok=0
  expect "bash -n" bash -n "$HAIL" || ok=1
  if command -v shellcheck >/dev/null 2>&1; then
    expect "shellcheck" shellcheck "$HAIL" || ok=1
  fi
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
  as "$SENDER" read worker 5 >/dev/null
  OUT=$(as "$SENDER" message worker "via alias" --kind fyi 2>/dev/null); RC=$?
  id=$(last_id)
  expect "message alias rc=0" eq "$RC" 0 || ok=1
  expect "delivered" exists "$INBOX/worker/$id.md" || ok=1
  as "$SENDER" read worker 5 >/dev/null
  OUT=$(as "$SENDER" msg worker "via msg" --kind fyi 2>/dev/null); RC=$?
  expect "msg alias rc=0" eq "$RC" 0 || ok=1
  reset_recv; return "$ok"
}

s20() { # tmux-bridge symlink + TMUX_BRIDGE_SOCKET fallback
  local ok=0 out
  expect "symlink version" eq "$("$SCRATCH/bin/tmux-bridge" version)" "hail 1.0.0" || ok=1
  out=$(env -u HAIL_SOCKET TMUX_BRIDGE_SOCKET="$HAIL_SOCKET" TMUX_PANE="$SENDER" "$SCRATCH/bin/tmux-bridge" resolve worker)
  expect "TMUX_BRIDGE_SOCKET fallback" eq "$out" "$RECV" || ok=1
  expect "read guard error names hail" contains "$(as "$SENDER" type worker x 2>&1)" "Run: hail read" || ok=1
  return "$ok"
}

scenario 1  "send from inside the pane: ruling, --body -, bead detected, 160-char envelope" s1
scenario 2  "bd present but failing: one warning, file-only, delivered" s2
scenario 3  "sent before read -> delivered" s3
scenario 4  "inbox --peek leaves no receipt" s4
scenario 5  "inbox writes receipt; sent -> read; --all" s5
scenario 6  "sent unknown id -> unknown" s6
scenario 7  "--kind stop typed inline in full" s7
scenario 8  "hyphenated words are not beads" s8
scenario 9  "--bead with --body file" s9
scenario 10 "--kind bogus rejected" s10
scenario 11 "labeled -> unlabeled pane keyed on pane id" s11
scenario 12 "fake bd comment id reported" s12
scenario 13 "no bd on PATH, --kind hold" s13
scenario 14 "version resolve id list doctor help" s14
scenario 15 "bash -n and shellcheck" s15
scenario 16 "await success" s16
scenario 17 "await timeout (no tmux server needed)" s17
scenario 18 "await --any" s18
scenario 19 "aliases message / msg" s19
scenario 20 "tmux-bridge symlink and TMUX_BRIDGE_SOCKET fallback" s20

echo "---"
echo "passed $PASS, failed $FAIL"
if (( FAIL > 0 )); then echo "failed scenarios: ${FAILED[*]}"; exit 1; fi
exit 0
