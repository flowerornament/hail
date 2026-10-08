---
title: "Quiet mail: progress off the pane, sub-agent addresses, holds that lapse"
date: 2026-10-08
status: shipped in 0.5.0 (§0 in 0.4.1); §1b removed in 0.5.1
---

# Quiet mail

On one busy coordinator over a day of work, about two thirds of the messages
it received were `fyi`, mostly progress reports. Each was typed into its pane
and cost it a turn. Prose rules ("hail carries decisions, never progress") did
not change the share, so the tool has to make the right path the easy one.

Three outcomes were asked for: progress never interrupts, sub-agents have an
address, and holds lapse. Investigation and review also found two data bugs.
Those shipped first, alone, as 0.4.1.

## 0. Bugs first: 0.4.1

- **An untyped headline-only message was lost.** `deliver` assumed every
  headline-only message was already in the prompt. One that was never typed
  (no pane, a dialog, typing not confirmed) was marked `injected` and never
  shown. Fix: `deliver` injects every headline-only message as its one-line
  envelope, whether or not it was typed. A duplicate line costs a few tokens;
  a loss costs the message.
  - *Rejected in review:* a "typed" marker written before Enter. It proves
    that Enter was pressed, not that the text arrived: a dialog opening after
    the guard, a cleared composer or dropped queued input would strand the
    message again.
- **`hail .. fyi x` wrote mail outside the store.** `Addr::parse` is now the
  one boundary and refuses any part that is empty, `.`, `..`, or contains `/`
  or NUL.

## 1. Progress never interrupts

**1a. An `fyi` is quiet when its recipient's hooks are known to run.** A quiet
`fyi` is written to the mailbox and not typed. It arrives with the recipient's
next prompt, so it costs them no turn.

- The typed headline was the fallback for a recipient whose hooks don't run
  (Codex hooks not yet trusted, or `hail setup` never run). So:
  - every `deliver` run as a hook (with `--format`) touches
    `seats/<addr>/hooked`;
  - a send makes an `fyi` quiet only if that file is less than 7 days old,
    and otherwise types it as before.

  That is one stat per send, and it calibrates itself.
- **Pending records:** a quiet `fyi` writes none. Its receipt comes whenever
  the recipient next works, so "no receipt after 2 min" would be noise.
- **Every actionable kind still types:** `ruling go nogo ask done stop hold
  block release announce`. Agents wait on them; a `done` often ends a wait.
  There is no `--wake` override: an `fyi` that needs a turn now is an `ask` or
  an `announce`.
- **Output:** the send exits 0 and prints `id=…` and then `quiet: arrives with
  <seat>'s next prompt`.
- **The skill says it plainly:** an agent waiting on a scheduled wakeup sees an
  `fyi` only at its next prompt, so a time-critical fact is never an `fyi`.

**1b. Progress on the issue (removed in 0.5.1).** 0.5.0 added `hail note
<issue> '<headline>'`, which posted progress as an issue-tracker comment and
messaged nobody. 0.5.1 removed it along with every other tie to an issue
tracker: the coupling tied hail to another tool's versions and failure modes
for little gain, since an agent can post to its tracker itself. The steering
that remains is in the skill: progress goes on the issue, and an `fyi` is for
outcomes the reader acts on later.

## 2. Sub-agents get an address: `<seat>/<name>`

A sub-agent has no pane and no hooks, and it runs in its parent's directory,
so it shares the parent's seat. Its parent already routes between its
sub-agents; hail makes that route addressable.

- **Target `web-1b/scout`:** the message goes to web-1b's mailbox with `for:
  scout`. The envelope shows `for:scout`, and the parent's pane is woken as
  the kind decides. The parent forwards it through its agent handle.
- **`--as <name>`:** a sub-agent signs as `seat/name`. `from:` and `reply:`
  read `web-1b/scout`, so the answer comes back through the parent.
- **Grammar:** `/` is split first, then `@`, and each part is checked at that
  one boundary (§0). A part beginning with `%` is a pane: `seat/%N` resolves
  exactly like the target `%N`, which already picks a sub-seat in a shared
  directory, plus a check that the pane is in that seat. This means a `from:`
  value can be pasted as a target. Any other part is a sub-agent name,
  following the seat-name rules. No seat name contains `/` or `@`, so nothing
  existing changes meaning.
- **Refusals:** an unknown target whose prefix is a known seat
  (`web-1b-scout`) is refused with `not a seat; if it is a sub-agent of
  web-1b, send to web-1b/scout (its parent relays)`.
- **The skill rule:** a sub-agent shares its parent's directory and
  environment, so it runs no verb that reads mail (`inbox`, `deliver`,
  `brief`); those would take the parent's mail and prune its pending sends.
  It sends, with `--as`.

## 3. Holds lapse

Holds never expired, so they accumulated: most recorded holds were more than
a day old, and some were weeks old. Meanwhile workflow tools (a gated land
command, host locks) had taken over serializing landing, installs and timing
runs, which is what most holds had been used for.

- **`--for <span>`** (`30m`, `8h`, `3d`) sets when a hold lapses. A `hold`
  defaults to 8h; a `block` defaults to 7d, because blocks report external
  blockers that outlive a session. Neither may exceed 7d. The record gains
  `expires:`, and the envelope shows `until:`.
- **Lapse is a read-time filter:** a hold whose `expires:` has passed is not in
  effect. A record without `expires:` lapses at `time:` plus the kind's
  default. Brief listings, counts and `release` all apply the filter, and no
  reader deletes anything.
- **The issuer is told once:** only the issuer's own `brief` moves its lapsed
  holds to `holds/lapsed/` and prints `lapsed: hold <id> …`. A `release` of a
  lapsed hold finds it there, moves nothing, and says it had lapsed. If any
  brief could retire a hold, the first one to run would swallow the issuer's
  notice.
- **`gc`** deletes `holds/lapsed/` entries past the archive age, and lapsed
  holds whose issuer has no mailbox left (issuers from 0.3, keyed by pane id,
  never run `brief` again).
- **Steering:** the skill's hold section opens with "a hold is a person's
  decision ('don't touch X while I redesign it'); to serialize landing,
  installs or timing runs, use the tool's lock."

## What changes for agents

- An `fyi` no longer interrupts an agent whose hooks run.
- `seat/name` reaches a sub-agent through its parent.
- Holds lapse after 8h, and blocks after 7d, unless given `--for`.
- The skill carries all of this, and agents pick it up at their next session
  start.

## Out of scope

- A digest or broadcast verb: a coordinator that wants a digest reads the
  issue tracker.
- Sub-agents receiving mail directly: that would need an identity hail cannot
  verify.
- Detecting progress by its content: a wrong guess is worse than none.

## Release

0.4.1 shipped §0. 0.5.0 shipped §1 to §3 in one CHANGELOG entry. Review of §3
added that a brief retires only as many lapsed holds as it shows, so the rest
are reported by a later brief rather than retired unseen. The
spec, DESIGN.md, the skill, `src/help.rs` and the scenarios changed together.
New scenarios:

- a quiet `fyi` types nothing and arrives with the next prompt;
- an `fyi` to a seat whose hooks have not run is typed;
- `seat/name` delivers to the parent with `for:`, and `--as` signs;
- `seat/%N` resolves like `%N`;
- a hold lapses for every reader, and is retired by its issuer's brief;
- `release` of a lapsed hold, and the `gc` sweep.
