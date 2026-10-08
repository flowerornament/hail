# hail hooks — Claude Code & Codex integration

With the hooks installed, a message body reaches the recipient on the turn its
envelope lands, as hook context, with no tool call; the receipt says
`injected <time>`. At session start the agent gets its brief: unread
envelopes, sends without receipt, holds and blocks in effect, open
obligations, five per section.

hail keeps no per-project state, so install the hooks once at user level and
every directory works without setup. `hail setup` does it for both harnesses
(it shows the change and asks first, and replaces older hail lines in place);
the blocks below are for doing it by hand. Both commands print nothing when
there is nothing to say, need no tmux server, and are silent outside a seat
(a directory with no jj workspace, git repo or `.hail-seat`), so no guard is
needed in front of them.

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
          { "type": "command", "command": "hail deliver --format claude" }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "hail brief --hook" }
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
hooks = [{ type = "command", command = "hail deliver --format codex" }]

[[hooks.SessionStart]]
hooks = [{ type = "command", command = "hail brief --hook" }]
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
          { "type": "command", "command": "hail deliver --format codex" }
        ]
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "hail brief --hook" }
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
| `SessionStart` | `hail brief --hook` | The brief as plain text (injected as context), or nothing. |

`deliver` claims each body once (a Maildir rename) and takes a few
milliseconds with nothing unread; it runs on
every prompt-like event (on Claude Code, task notifications too). The
injected body is not retained through compaction; the file under
`~/.local/state/hail/seats/<seat>/` is.

Both harnesses read hooks at session start; installing or changing them needs
a new session.

## Verify by firing

Start a fresh session inside tmux, then from another pane:

```console
$ hail <seat> fyi 'hook check'
$ hail sent <id>            # injected <time> once the recipient's next turn ran the hook
```

If it stays `delivered`, the hook did not run: on Codex the usual cause is
pending review in `/hooks`.
