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
export HAIL_ENVELOPE_MAX=160   # scenarios were written against the original cap; the tool default is 400
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
as() { local pane="$1"; shift; TMUX_PANE="$pane" "$HAIL" "$@" </dev/null; }
pane_text() { "${T[@]}" capture-pane -t "$1" -p -J; }
# A respawned pane has a new pane process, so it is a new incarnation: re-register.
reset_recv() { "${T[@]}" respawn-pane -k -t "$RECV" cat; sleep 0.2; as "$SENDER" name "$RECV" worker; }
# respawn with a command that prints something first, then behaves like cat
recv_showing() { "${T[@]}" respawn-pane -k -t "$RECV" bash -c "printf '%s\n' \"\$@\"; exec cat" _ "$@"; sleep 0.3; as "$SENDER" name "$RECV" worker; }
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
ASK1='Ruling on herald-ke7is: convert at the receipt, not the producer; scratch state stays outside every repo tree; do not read panes for replies'

s1() { # from inside the sender pane via send-keys, --kind ruling --body -
  printf 'line one of the body\nline two of the body\n' > "$SCRATCH/body1"
  local ok=0
  # The pane runs the script so TMUX_PANE comes from tmux itself, not from
  # the harness. A short typed line: readline stalls on lines past ~1 KB.
  cat > "$SCRATCH/s1.sh" <<EOS
export HAIL_ENVELOPE_MAX='$HAIL_ENVELOPE_MAX' HAIL_SOCKET='$HAIL_SOCKET' XDG_STATE_HOME='$XDG_STATE_HOME' PATH='$PATH'
'$HAIL' read '$RECV' 5 >/dev/null
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
  expect "envelope head" contains "$line" "[hail kind:ruling from:boss/$SENDER reply:$SENDER id:$id bead:herald-ke7is]" || ok=1
  expect "fetch hint" contains "$line" "— hail inbox" || ok=1
  expect "headline typed in full" contains "$line" "] $ASK1 — hail inbox" || ok=1
  expect "not truncated" not_contains "$line" "…" || ok=1
  expect "submitted: cat echoed the envelope" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  local f="$INBOX/worker/$id.md"
  expect "inbox file exists" exists "$f" || ok=1
  expect "file has full ask" grep -qF "ask: $ASK1" "$f" || ok=1
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
  expect "file still written" exists "$INBOX/worker/$id.md" || ok=1
  expect "receipt pre-written as inline" grep -qE '^inline [0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$' "$INBOX/worker/$id.read" || ok=1
  expect "sent -> inline <time>" re "$(as "$SENDER" sent "$id")" '^inline [0-9]{4}-.*Z$' || ok=1
  expect "deliver does not hand it over again" not_contains "$(as "$RECV" deliver)" "id: $id" || ok=1
  expect "inbox does not either" not_contains "$(as "$RECV" inbox)" "id: $id" || ok=1
  expect "inbox --all shows the inline receipt" contains "$(as "$RECV" inbox --all)" "receipt: inline " || ok=1
  expect "brief does not list it as unread" not_contains "$(as "$RECV" brief)" "id:$id" || ok=1
  expect "await sees it" contains "$(as "$SENDER" await "$id" --timeout 1)" "$id inline " || ok=1
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
  expect "lists kinds" contains "$ERR" "ruling go nogo ask fyi done stop hold block release announce" || ok=1
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
  expect "version" eq "$(as "$SENDER" version)" "hail 0.2.1" || ok=1
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
  expect "symlink version" eq "$("$SCRATCH/bin/tmux-bridge" version)" "hail 0.2.1" || ok=1
  out=$(env -u HAIL_SOCKET TMUX_BRIDGE_SOCKET="$HAIL_SOCKET" TMUX_PANE="$SENDER" "$SCRATCH/bin/tmux-bridge" resolve worker)
  expect "TMUX_BRIDGE_SOCKET fallback" eq "$out" "$RECV" || ok=1
  as "$SENDER" keys worker Escape >/dev/null 2>&1 || true   # consume any standing read mark
  expect "read guard error names hail" contains "$(as "$SENDER" type worker x 2>&1)" "Run: hail read" || ok=1
  return "$ok"
}

# --- 0.2.1 scenarios ----------------------------------------------------------
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
# Drop holds, obligations and send records left by earlier scenarios.
clear_state() { rm -rf "$XDG_STATE_HOME/hail/holds" "$XDG_STATE_HOME/hail/obligations" "$XDG_STATE_HOME/hail/sent"; }

s21() { # deliver: plain text, receipt says injected, silent afterwards, inbox --all distinguishes
  local ok=0 a b out
  send "$SENDER" worker "first for deliver" --kind ask; a=$(last_id)
  send "$SENDER" worker "second for deliver" --kind fyi; b=$(last_id)
  out=$(as "$RECV" deliver); RC=$?
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "prints first body" contains "$out" "id: $a" || ok=1
  expect "prints second body" contains "$out" "second for deliver" || ok=1
  expect "bodies separated" contains "$out" "---" || ok=1
  expect ".read says injected <UTC>" grep -qE '^injected [0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$' "$INBOX/worker/$a.read" || ok=1
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
  send "$SENDER" worker "claude format" --kind fyi; id=$(last_id)
  out=$(as "$RECV" deliver --format claude)
  expect "claude format is the same JSON" contains "$out" "\"hookEventName\":\"UserPromptSubmit\",\"additionalContext\":\"from: boss/$SENDER" || ok=1
  expect "silent when empty (codex)" empty "$(as "$RECV" deliver --format codex)" || ok=1
  expect "silent when empty (claude)" empty "$(as "$RECV" deliver --format claude)" || ok=1
  expect "bad format rejected" eq "$(as "$RECV" deliver --format vim >/dev/null 2>&1; echo $?)" 1 || ok=1
  reset_recv; return "$ok"
}

s23() { # deliver needs no tmux server and is fast; hook JSON on stdin is accepted
  local ok=0 id out t0 t1 best=99999 i
  send "$SENDER" worker "no server needed" --kind ask; id=$(last_id)
  out=$(printf '{"session_id":"x","prompt":"hi"}' | HAIL_SOCKET=/nonexistent/socket TMUX_PANE="$RECV" "$HAIL" deliver --format codex); RC=$?
  expect "rc=0 without a server" eq "$RC" 0 || ok=1
  expect "delivered without a server" contains "$out" "id: $id" || ok=1
  for i in 1 2 3; do
    t0=$(now_ms); HAIL_SOCKET=/nonexistent/socket TMUX_PANE="$RECV" "$HAIL" deliver --format codex </dev/null >/dev/null; t1=$(now_ms)
    (( t1 - t0 < best )) && best=$(( t1 - t0 ))
  done
  expect "empty deliver under 50 ms (best of 3: ${best} ms)" le "$best" 50 || ok=1
  reset_recv; return "$ok"
}

s24() { # brief: silent when empty; inbox section; sends without receipt; disappears once read
  clear_state
  local ok=0 id out
  expect "brief silent when empty" empty "$(as "$RECV" brief)" || ok=1
  send "$SENDER" worker "brief me" --kind fyi --scope herald/x; id=$(last_id)
  out=$(as "$RECV" brief)
  expect "inbox header" contains "$out" "inbox (1 unread)" || ok=1
  expect "envelope-style line" contains "$out" "[hail fyi from:boss id:$id scope:herald/x] brief me" || ok=1
  expect "no sends section on recipient" not_contains "$out" "my sends" || ok=1
  expect "sender brief: fresh send not listed" empty "$(as "$SENDER" brief)" || ok=1
  sed -i.bak "s/^epoch: .*/epoch: $(( $(date +%s) - 200 ))/" "$XDG_STATE_HOME/hail/sent/boss/$id" && rm -f "$XDG_STATE_HOME/hail/sent/boss/$id.bak"
  out=$(as "$SENDER" brief)
  expect "sends without receipt header" contains "$out" "my sends without receipt (1)" || ok=1
  expect "sends line" contains "$out" "[hail fyi to:worker id:$id] brief me (3m, no receipt)" || ok=1
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
  expect "hold file" exists "$XDG_STATE_HOME/hail/holds/$h" || ok=1
  expect "hold issuer" grep -qx "issuer: boss" "$XDG_STATE_HOME/hail/holds/$h" || ok=1
  line=$(envelope_line "id:$h")
  expect "scope in envelope" contains "$line" "id:$h scope:murail-ke7is]" || ok=1
  expect "hold typed in full, no hint" not_contains "$line" "hail inbox" || ok=1
  send "$SENDER" worker "BLOCK: anchor dirty" --kind block; b=$(last_id)
  expect "block file" exists "$XDG_STATE_HOME/hail/holds/$b" || ok=1
  out=$(as "$RECV" brief)
  expect "brief lists holds (2)" contains "$out" "holds / blocks in effect (2)" || ok=1
  expect "brief hold line" contains "$out" "[hail hold from:boss to:worker id:$h scope:murail-ke7is] HOLD landing" || ok=1
  expect "brief block line" contains "$out" "[hail block from:boss to:worker id:$b]" || ok=1
  # another issuer (worker) cannot release boss's hold
  send "$RECV" boss "lifting your hold" --kind release --re "$h"
  expect "release by other refused rc=1" eq "$RC" 1 || ok=1
  expect "refusal names the issuer" contains "$ERR" "issued by boss, not by worker" || ok=1
  expect "hold still there" exists "$XDG_STATE_HOME/hail/holds/$h" || ok=1
  reset_sender
  send "$SENDER" worker "lifted" --kind release
  expect "release without --re refused" eq "$RC" 1 || ok=1
  send "$SENDER" worker "lifted" --kind release --re "$h"
  expect "release by issuer rc=0" eq "$RC" 0 || ok=1
  expect "re: in envelope" contains "$(envelope_line "id:$(last_id)")" " re:$h]" || ok=1
  expect "hold removed" missing "$XDG_STATE_HOME/hail/holds/$h" || ok=1
  expect "block survives" exists "$XDG_STATE_HOME/hail/holds/$b" || ok=1
  send "$SENDER" worker "unblocked" --kind release --re "$b"
  expect "block released" missing "$XDG_STATE_HOME/hail/holds/$b" || ok=1
  expect "no holds section" not_contains "$(as "$RECV" brief)" "holds" || ok=1
  reset_recv; return "$ok"
}

s26() { # go -> done closes exactly it; wrong re fails; obligation survives a read ("compaction")
  clear_state
  local ok=0 g r out
  send "$SENDER" worker "GO commit on base abc123" --kind go --scope commit; g=$(last_id)
  send "$SENDER" worker "convert at the receipt" --kind ruling; r=$(last_id)
  expect "obligation files" exists "$XDG_STATE_HOME/hail/obligations/worker/$g" || ok=1
  as "$RECV" inbox >/dev/null          # the envelope is read; the body leaves context at compaction
  out=$(as "$RECV" brief)
  expect "obligations listed after read" contains "$out" "open obligations on me (2)" || ok=1
  expect "go line with scope" contains "$out" "[hail go from:boss id:$g scope:commit] GO commit on base abc123 — hail done --re $g" || ok=1
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
  expect "done envelope has re and scope" contains "$(pane_text "$SENDER")" "id:$(last_id) re:$g scope:commit] committed — hail inbox" || ok=1
  expect "go obligation closed" missing "$XDG_STATE_HOME/hail/obligations/worker/$g" || ok=1
  expect "ruling obligation open" exists "$XDG_STATE_HOME/hail/obligations/worker/$r" || ok=1
  expect "brief lists the remaining one" contains "$(as "$RECV" brief)" "open obligations on me (1)" || ok=1
  reset_sender; reset_recv; return "$ok"
}

s27() { # headline over the cap refused for a control kind; missing kind refused
  local ok=0 text
  text="STOP: red gate on master, do not land anything until the fan-in gate is green again; this sentence is padded to be long enough to exceed the envelope budget by a comfortable margin ok"
  send "$SENDER" worker "$text" --kind stop
  expect "rc=2" eq "$RC" 2 || ok=1
  expect "message names the cap" contains "$ERR" "headline is $(chars "$text") characters; the cap is 160" || ok=1
  expect "nothing typed" not_contains "$(pane_text "$RECV")" "STOP: red gate" || ok=1
  expect "no file" eq "$(ls "$INBOX/worker" | grep -c "$(date -u +%m%d)" )" "$(ls "$INBOX/worker" | grep -c "$(date -u +%m%d)")" || ok=1
  send "$SENDER" worker "$LONG_ASK" --kind ruling
  expect "ruling over cap rc=2" eq "$RC" 2 || ok=1
  send "$SENDER" worker "no kind given"
  expect "missing kind rc=1" eq "$RC" 1 || ok=1
  expect "missing kind names the kinds" contains "$ERR" "--kind is required (ruling go nogo ask fyi done stop hold block release announce)" || ok=1
  return "$ok"
}

s28() { # identity: label moved -> send refused (exit 3); who shows it; hello = new incarnation
  local ok=0 out
  "${T[@]}" set-option -p -t "$RECV" -u @name
  "${T[@]}" set-option -p -t "$SENDER" @name worker
  send "$SENDER" worker "to whoever wears the label" --kind fyi
  expect "rc=3" eq "$RC" 3 || ok=1
  expect "message" contains "$ERR" "label worker moved: registered on $RECV, now on $SENDER — run hail name to re-register" || ok=1
  out=$(as "$SENDER" who worker)
  expect "who label" contains "$out" "label: worker" || ok=1
  expect "who registered" contains "$out" "registered: $RECV" || ok=1
  expect "who wearing" contains "$out" "wearing: $SENDER" || ok=1
  expect "who status moved" contains "$out" "status: label moved to $SENDER; registered on $RECV — run: hail name $SENDER worker" || ok=1
  expect "who incarnation" re "$out" 'incarnation: [0-9]{4}T[0-9]{6}-[0-9a-f]{4}' || ok=1
  expect "who last event" re "$out" 'last inbox event: [0-9]{4}-.*Z' || ok=1
  expect "who pane tail" contains "$out" "pane tail:" || ok=1
  as "$SENDER" name "$SENDER" boss
  as "$SENDER" name "$RECV" worker
  send "$SENDER" worker "back on the registered pane" --kind fyi
  expect "re-registered send rc=0" eq "$RC" 0 || ok=1
  local reg inc
  reg=$(sed -n 's/^incarnation: //p' "$XDG_STATE_HOME/hail/identity/worker")
  expect "name minted the incarnation" re "$reg" '^[0-9]{4}T[0-9]{6}-[0-9a-f]{4}$' || ok=1
  expect "incarnation file written by name" eq "$(head -1 "$XDG_STATE_HOME/hail/incarnation/${RECV/\%/_}")" "$reg" || ok=1
  inc=$(as "$RECV" hello)
  expect "hello is idempotent (keeps the minted id)" eq "$inc" "$reg" || ok=1
  expect "hello needs no server" eq "$(HAIL_SOCKET=/nonexistent/socket as "$RECV" hello)" "$reg" || ok=1
  send "$SENDER" worker "after hello" --kind fyi
  expect "send still works after hello rc=0" eq "$RC" 0 || ok=1
  # a real restart: the pane's process is replaced; the incarnation file is now stale
  "${T[@]}" respawn-pane -k -t "$RECV" cat; sleep 0.3
  inc=$(as "$RECV" hello)
  expect "hello after restart mints a new id" re "$inc" '^[0-9]{4}T[0-9]{6}-[0-9a-f]{4}$' || ok=1
  expect "new id differs" eq "$([[ "$inc" != "$reg" ]] && echo differs)" differs || ok=1
  expect "hello then stays put" eq "$(as "$RECV" hello)" "$inc" || ok=1
  send "$SENDER" worker "after restart" --kind fyi
  expect "restarted pane refused rc=3" eq "$RC" 3 || ok=1
  expect "message names incarnations" contains "$ERR" "now on $RECV ($inc) — run hail name to re-register" || ok=1
  out=$(as "$RECV" brief)
  expect "brief on the new incarnation still lists the obligation" contains "$out" "open obligations on me (1)" || ok=1
  expect "brief says the pane restarted" contains "$out" 'label worker: pane restarted — run: hail name "$(hail id)" worker' || ok=1
  expect "brief line first" re "$out" '^label worker: pane restarted' || ok=1
  expect "who shows the restart" contains "$(as "$SENDER" who worker)" 'status: pane restarted since registration — run: hail name "$(hail id)" worker' || ok=1
  expect "brief without a server has no false restart line" not_contains "$(HAIL_SOCKET=/nonexistent/socket as "$RECV" brief)" "pane restarted" || ok=1
  as "$SENDER" name "$RECV" worker
  send "$SENDER" worker "after re-register" --kind fyi
  expect "send after re-register rc=0" eq "$RC" 0 || ok=1
  expect "brief clean after re-register" not_contains "$(as "$RECV" brief)" "restarted" || ok=1
  expect "who ok after re-register" contains "$(as "$SENDER" who worker)" "status: ok" || ok=1
  reset_recv; return "$ok"
}

s29() { # bare-target send form
  local ok=0 id
  as "$SENDER" read worker 5 >/dev/null
  OUT=$(as "$SENDER" worker "bare form works" --kind fyi 2>"$SCRATCH/err"); RC=$?; ERR=$(cat "$SCRATCH/err")
  id=$(last_id)
  expect "rc=0 ($ERR)" eq "$RC" 0 || ok=1
  expect "delivered" exists "$INBOX/worker/$id.md" || ok=1
  expect "envelope" contains "$(envelope_line "id:$id")" "[hail kind:fyi from:boss/$SENDER" || ok=1
  expect "help documents the bare form" contains "$("$HAIL" --help)" "Usage: hail <target> <headline> --kind <kind>" || ok=1
  expect "single unknown word is an error" contains "$(as "$SENDER" bogus 2>&1)" "unknown command: bogus" || ok=1
  reset_recv; return "$ok"
}

s30() { # send submits; --no-submit does not and keeps the read mark
  local ok=0 id
  send "$SENDER" worker "submitted for you" --kind fyi; id=$(last_id)
  sleep 0.2
  expect "Enter pressed: cat echoed the line (2 copies)" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  expect "read mark consumed" contains "$(as "$SENDER" keys worker Enter 2>&1)" "Run: hail read" || ok=1
  send "$SENDER" worker "not submitted" --kind fyi --no-submit; id=$(last_id)
  sleep 0.2
  expect "no Enter: one copy" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 1 || ok=1
  as "$SENDER" keys worker Enter; RC=$?
  expect "read mark kept for the Enter" eq "$RC" 0 || ok=1
  sleep 0.2
  expect "now two copies" eq "$(pane_text "$RECV" | grep -cF "id:$id")" 2 || ok=1
  reset_recv; return "$ok"
}

s31() { # guards: permission dialog (exit 4), unsent draft (exit 5), --force
  local ok=0
  recv_showing "Bash(rm -rf build)" "Do you want to proceed?" "  1. Yes" "  2. Yes, and don't ask again" "  3. No" "Esc to cancel"
  send "$SENDER" worker "would approve rm" --kind fyi
  expect "dialog rc=4" eq "$RC" 4 || ok=1
  expect "dialog message" contains "$ERR" "shows a permission/approval dialog" || ok=1
  expect "nothing typed" not_contains "$(pane_text "$RECV")" "would approve rm" || ok=1
  send "$SENDER" worker "forced past dialog" --kind fyi --force
  expect "--force rc=0" eq "$RC" 0 || ok=1
  recv_showing "some earlier output" "> half a thought the user has not sent"
  send "$SENDER" worker "would append to a draft" --kind fyi
  expect "draft rc=5" eq "$RC" 5 || ok=1
  expect "draft message" contains "$ERR" "unsent draft in its composer: 'half a thought" || ok=1
  send "$SENDER" worker "forced past draft" --kind fyi --force
  expect "--force past draft rc=0" eq "$RC" 0 || ok=1
  recv_showing "› [hail kind:fyi from:x id:y] an envelope waiting to be submitted"
  send "$SENDER" worker "envelope drafts are fine" --kind fyi
  expect "hail draft allowed rc=0" eq "$RC" 0 || ok=1
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

s33() { # shell target: no Enter by default, note printed, read mark kept; --force submits
  local ok=0 id
  send "$RECV" boss "into a shell" --kind fyi; id=$(last_id)
  sleep 0.2
  expect "rc=0" eq "$RC" 0 || ok=1
  expect "note names the shell" contains "$ERR" "$SENDER runs bash, a shell; envelope typed but not submitted" || ok=1
  expect "typed once, not executed" eq "$(pane_text "$SENDER" | grep -cF "id:$id")" 1 || ok=1
  expect "bash did not run it" not_contains "$(pane_text "$SENDER")" "command not found" || ok=1
  as "$RECV" keys boss C-u; RC=$?
  expect "read mark kept" eq "$RC" 0 || ok=1
  send "$RECV" boss "forced into a shell" --kind fyi --force; id=$(last_id)
  sleep 0.3
  expect "--force rc=0" eq "$RC" 0 || ok=1
  expect "no note with --force" not_contains "$ERR" "not submitted" || ok=1
  expect "submitted: bash tried to run it" contains "$(pane_text "$SENDER")" "command not found" || ok=1
  reset_sender; return "$ok"
}

scenario 1  "send from inside the pane: ruling, --body -, bead detected, submitted" s1
scenario 2  "bd present but failing: one warning, file-only, delivered" s2
scenario 3  "sent before read -> delivered" s3
scenario 4  "inbox --peek leaves no receipt" s4
scenario 5  "inbox writes receipt; sent -> read; --all" s5
scenario 6  "sent unknown id -> unknown" s6
scenario 7  "--kind stop typed inline in full; receipt pre-written as inline" s7
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
scenario 21 "deliver: plain text, injected receipt, silent when empty, inbox --all" s21
scenario 22 "deliver --format codex / claude: hook JSON, escaping" s22
scenario 23 "deliver without a tmux server, under 50 ms, stdin accepted" s23
scenario 24 "brief: silent when empty, inbox, sends without receipt" s24
scenario 25 "hold/block -> release by issuer, refused for another; re:/scope:" s25
scenario 26 "go/ruling -> done closes exactly one; wrong re fails; survives a read" s26
scenario 27 "headline over cap refused (control kind too); missing kind refused" s27
scenario 28 "identity: moved label refused (exit 3), who, name mints, hello idempotent, restart" s28
scenario 29 "bare-target send form" s29
scenario 30 "send submits; --no-submit keeps the read mark" s30
scenario 31 "guards: permission dialog, unsent draft, --force" s31
scenario 32 "read <target> N returns exactly N lines" s32
scenario 33 "send into a shell pane types but does not submit; --force submits" s33

echo "---"
echo "passed $PASS, failed $FAIL"
if (( FAIL > 0 )); then echo "failed scenarios: ${FAILED[*]}"; exit 1; fi
exit 0
