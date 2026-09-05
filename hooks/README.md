# hail hooks — Claude Code & Codex integration

With the hooks installed, a message body reaches the recipient on the turn its
envelope lands, as hook context, with no tool call; the receipt says
`injected <time>`. At session start the agent gets its brief: unread
envelopes, sends without receipt, holds and blocks in effect, open
obligations, and a one-line notice if its label needs re-registration.

hail keeps no per-project state, so install the hooks once at user level and
every directory works without setup. Both commands print nothing when there
is nothing to say and need no tmux server; `$TMUX_PANE` is inherited from the
pane the agent runs in, and the `[ -n "$TMUX_PANE" ]` guard keeps the hook
silent outside tmux.

## Without hooks

Everything still works, one step slower: the envelope lands in the prompt,
the agent runs `hail inbox` to fetch bodies and write `read <time>` receipts,
and `hail brief` on demand shows the standing state. Control kinds are
complete in the envelope either way.

## User-level install

### Claude Code — `~/.claude/settings.json`

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

Merge into the existing file; other keys and other tools' hooks stay.

### Codex — `~/.codex/config.toml`

```toml
[features]
hooks = true

[[hooks.UserPromptSubmit]]
hooks = [{ type = "command", command = "[ -n \"$TMUX_PANE\" ] || exit 0; hail deliver --format codex" }]

[[hooks.SessionStart]]
hooks = [{ type = "command", command = "[ -n \"$TMUX_PANE\" ] || exit 0; hail brief" }]
```

The `[[hooks.<Event>]]` tables carry the same `hooks = [...]` array as the
JSON form; event names are the same as in `hooks.json` (`UserPromptSubmit`,
`SessionStart`). Codex still gates every command hook behind trust: run
`/hooks` in the Codex CLI once to review and trust them, even at user level.
Trust is recorded against the hash of the hook text, so it holds until the
command changes. Non-interactive automation can pass
`codex exec --dangerously-bypass-hook-trust`.

## Per-project override

The same blocks in a project's `.claude/settings.json` (or
`.claude/settings.local.json`, git-ignored) and `.codex/hooks.json`:

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

(`--format claude` in the Claude file.) Codex loads project hooks only in a
trusted project, and `/hooks` trust is per hook definition, so a project copy
needs its own review. A layer carrying both `.codex/hooks.json` and
`[[hooks.*]]` in `.codex/config.toml` loads both and warns; use one.

## What each hook does

| event | command | output |
|---|---|---|
| `UserPromptSubmit` | `hail deliver --format <harness>` | `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"<unread bodies>"}}`, or nothing. Marks each body `injected <UTC time>`. Control kinds are complete in the envelope and are not injected. |
| `SessionStart` | `hail brief` | The brief as plain text (injected as context), or nothing. |

`deliver` is idempotent and takes about 15 ms with nothing unread; it runs on
every prompt-like event (on Claude Code, task notifications too). The
injected body is not retained through compaction; the file under
`~/.local/state/hail/inbox/` and the bead comment are.

The hook does not run `hail hello`: Codex fires SessionStart at the first
prompt, after `hail name` has registered the label, and `hail name` mints the
pane's incarnation itself. After a real restart of the pane's process the
brief prints `label <l>: pane restarted — run: hail name "$(hail id)" <l>`.

Both harnesses read hooks at session start; installing or changing them needs
a new session.

## Verify by firing

Start a fresh session inside tmux, then from another pane:

```console
$ hail read <label> 5
$ hail <label> 'hook check' --kind fyi
$ hail sent <id>            # injected <time> once the recipient's next turn ran the hook
```

If it stays `delivered`, the hook did not run: on Codex the usual cause is
pending review in `/hooks`.
