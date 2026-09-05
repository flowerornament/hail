# hail hooks — Claude Code & Codex integration

With the hooks installed, a message body reaches the recipient on the turn its
envelope lands, as hook context, with no tool call; the receipt says
`injected <time>`. At session start the agent gets its brief: unread envelopes,
sends without receipt, holds and blocks in effect, open obligations.

The snippets below are copied config: paste them into the project's file and
merge with any hooks already there (peat's, `bd prime`, formatters). Both
commands print nothing when there is nothing to say, and neither needs a tmux
server: `$TMUX_PANE` is inherited from the pane the agent runs in. The
`[ -n "$TMUX_PANE" ]` guard keeps the hook silent when the agent is not in tmux.

## Claude Code — `.claude/settings.json`

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail deliver --format claude" }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail brief" }
        ]
      }
    ]
  }
}
```

`.claude/settings.local.json` takes the same shape for a per-user, git-ignored
install.

## Codex — `.codex/hooks.json`

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail deliver --format codex" }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "[ -n \"$TMUX_PANE\" ] || exit 0; hail brief" }
        ]
      }
    ]
  }
}
```

## What each hook does

| event | command | output |
|---|---|---|
| `UserPromptSubmit` | `hail deliver --format <harness>` | `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"<unread bodies>"}}`, or nothing. Marks each body `injected <UTC time>`. |
| `SessionStart` | `hail brief` | The brief as plain text (injected as context), or nothing. |

`deliver` is idempotent and takes about 15 ms with nothing unread; it runs on
every prompt-like event (on Claude Code, task notifications too). The
injected body is not retained through compaction; the file under
`~/.local/state/hail/inbox/` and the bead comment are.

`hail name` mints the pane's incarnation; the hook does not run `hail hello`
(Codex fires SessionStart at the first prompt, after `name`), and `hello` never
replaces an incarnation the pane already has. After a real restart the brief
prints `label <l>: pane restarted — run: hail name "$(hail id)" <l>`.

## Codex: hooks must be trusted before they run

Codex records trust against the hash of the exact hook definition and skips
an untrusted hook silently. Two gates, both required:

1. Project trust: project-local hooks load only when the project's `.codex/`
   layer is trusted.
2. Hook trust: run `/hooks` in the Codex CLI to review and trust new or
   changed hooks. Codex prints a warning at startup while review is pending.

This is one review per project for as long as the command text stays the same.
Non-interactive automation can use `codex exec --dangerously-bypass-hook-trust`.

Both harnesses read hooks at session start; installing or changing them needs
a new session.

## Verify by firing

Start a fresh session in the project inside tmux, then from another pane:

```console
$ hail read <label> 5
$ hail <label> 'hook check' --kind fyi
$ hail sent <id>            # injected <time> once the recipient's next turn ran the hook
```

If it stays `delivered`, the hook did not run: on Codex the usual cause is
pending review in `/hooks`.
