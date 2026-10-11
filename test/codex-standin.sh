# A stand-in for Codex's screen in the scenarios, run as `codex` (a symlink to
# bash, so tmux and hail see a Codex pane). It draws a transcript, the held
# sections Codex shows below it, a `› ` composer and a footer, reads the
# terminal raw, and on Enter does what MODE says:
#   submit         the text joins the transcript; the composer empties
#   ignore-once    the first Enter does nothing, the second submits
#   ignore         Enter never takes; the text stays in the composer
#   steer | end-of-turn | queued   the text waits in that held section
# A second argument, goal, adds "Pursuing goal" to the footer.
mode="$1" goal="${2:-}"
history=("• earlier output") held=() buf="" enters=0
draw() {
  printf '\033[2J\033[H'
  local l
  for l in "${history[@]}" "${held[@]}"; do printf '%s\r\n' "$l"; done
  if [ -n "$buf" ]; then printf '› %s\r\n' "$buf"; else printf '› Ask Codex to do anything\r\n'; fi
  printf '  stand-in · %s\r\n  ← for agents · ? for shortcuts' "${goal:+Pursuing goal (4m)}"
}
hold() { held=("$1" "  ↳ $buf"); buf=""; }
stty raw -echo
draw
while IFS= read -r -n1 -d '' c; do
  # bash's read -n sets the terminal itself, so Enter can arrive as \r, as
  # \n, or as an empty read.
  if [ "$c" = $'\r' ] || [ "$c" = $'\n' ] || [ -z "$c" ]; then
    enters=$((enters + 1))
    case "$mode" in
      submit) history+=("› $buf" "• Working (1s • esc to interrupt)"); buf="" ;;
      ignore-once) [ "$enters" -ge 2 ] && { history+=("› $buf" "• Working (1s • esc to interrupt)"); buf=""; } ;;
      ignore) ;;
      steer) hold "• Messages to be submitted after next tool call" ;;
      end-of-turn) hold "• Messages to be submitted at end of turn" ;;
      queued) hold "• Queued follow-up inputs" ;;
    esac
  elif [ "$c" = $'\033' ]; then
    # Escape interrupts a Codex turn: hail must never send it.
    history+=("• ESC RECEIVED")
  else
    buf+="$c"
  fi
  draw
done
