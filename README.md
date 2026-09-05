# hail

Agent-to-agent messaging for coding agents that share a machine, over tmux.
A `send` writes the body to a file and types a one-line `[hail ...]` envelope
into the recipient's prompt; the recipient's own `hail inbox` fetches the body
and writes the receipt; `hail sent` and `hail await` read that receipt. Nobody
reads a pane to find out whether a message landed. No daemon; state is files
under `$XDG_STATE_HOME/hail`.

Design and rationale: [DESIGN.md](DESIGN.md). Agent instructions: [skills/hail/SKILL.md](skills/hail/SKILL.md).

## Install (nix overlay)

`package.nix` is flake-free. In an overlay:

```nix
(final: prev: {
  hail = final.callPackage /path/to/hail/package.nix { };
})
```

then add `hail` to your packages. It installs `bin/hail`, a `tmux-bridge`
symlink (compatibility, one week), and the skill at `share/hail/skills/hail`.
Requires `tmux` at runtime; `fswatch` is optional (`await` polls without it).

## The three commands an agent needs

```bash
hail send murail-1b 'convert at the receipt' --kind ruling --bead murail-ke7is --body ruling.md
#   → id=0905T1712-a3f1
hail inbox                              # recipient: print unread bodies, write receipts
hail await 0905T1712-a3f1 --timeout 600 # sender: block until it is read (or: hail sent <id>)
```

Label panes first (`hail name "$(hail id)" murail-1b`); inboxes are keyed on labels.
`hail --help` lists every verb. Tests: `test/run.sh` (scratch tmux server only).
