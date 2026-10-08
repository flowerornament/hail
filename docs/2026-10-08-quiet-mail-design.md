---
title: "Quiet mail: progress off the pane, sub-agent addresses, holds that lapse"
date: 2026-10-08
status: shipped in 0.5.0 (§0 in 0.4.1)
asked_by: murail-2a, with the user's direction (hail id 1008T015425-0c48)
reviews:
  - murail-2a, no nogo, three notes (1008T015735-8195)
  - critic hail@%28, change four things (1008T020207-cb27; hail-61k, hail-ani, hail-b28, hail-89h)
---

# Quiet mail

Murail's census for 2026-10-07 to 10-08 counted 667 messages to the
coordinator, 455 of them `fyi` (68%), mostly progress. Each was typed into a
pane and cost the coordinator a turn. Prose rules ("hail carries decisions,
never progress") did not move the number, so the tool has to make the right
path the easy one.

Three outcomes were asked for. Investigation and review turned up two data
bugs as well, and those ship first, alone, as 0.4.1.

## 0. Bugs first: 0.4.1

- **hail-2en: an untyped headline-only message was lost.** `deliver`
  assumed every headline-only message was already in the prompt. One never
  typed (no pane, a dialog, not confirmed) was marked `injected` and never
  shown. Fix: `deliver` injects every headline-only message as its one-line
  envelope, whether or not it was typed. A duplicate line costs a few tokens;
  a loss costs the message.
  - *Rejected in review:* a typed marker written before Enter. It proves that
    Enter was pressed, not that the text arrived: a dialog opening after the
    guard, a cleared composer or dropped queued input would strand the message
    again.
- **hail-cr9: `hail .. fyi x` wrote mail outside the store.** `Addr::parse` is
  now the one boundary and refuses any part that is empty, `.`, `..`, or
  contains `/` or NUL.

## 1. Progress never interrupts

**1a. An `fyi` is quiet when its recipient's hooks are known to run.** A quiet
`fyi` is written to the mailbox and not typed. It arrives with the recipient's
next prompt, so it costs them no turn.

- The typed headline was the fallback for a recipient whose hooks don't run
  (Codex hooks untrusted, `nx-rs` has never had one delivery). So:
  - every `deliver` touches `seats/<addr>/hooked`;
  - a send makes an `fyi` quiet only if that file is less than 7 days old,
    and otherwise types it as today.

  That is one stat per send, and it calibrates itself.
- **Pending records:** a quiet `fyi` writes none. Its receipt comes whenever
  the recipient next works, so "no receipt after 2 min" would be noise.
- **Every actionable kind still types:** `ruling go nogo ask done stop hold
  block release announce`. Seats park on them; a `done` often ends a wait.
  There is no `--wake` override: an `fyi` that needs a turn now is an `ask` or
  an `announce`.
- **Output:** the send exits 0 and prints `id=…` and then `quiet: arrives with
  <seat>'s next prompt`.
- **The skill says it plainly:** a seat parked on a scheduled wakeup sees an
  `fyi` only at its next prompt, so a time-critical fact is never an `fyi`.

**1b. `hail note <bead> '<headline>' [<<body]` puts progress on the bead.** It
posts a bd comment signed with the seat (`[murail-2b] headline`, then the
body), and nothing else: no mailbox, no pane, no receipt. It costs nobody
anything until someone reads the bead.

- If bd fails, the note exits 1 and quotes bd's own error, because the bead
  holds the only copy.
- bd runs in the caller's directory, so a jj workspace whose `.beads` is a
  redirect reaches the shared database.

**Steering, without guessing at content.** The skill and help say that
progress goes to `hail note`, and that an `fyi` is for outcomes the reader
acts on later. An `fyi` that names a bead prints one hint on stderr: `hail
note <bead> posts this without a message`. The census measures the result.

## 2. Sub-agents get an address: `<seat>/<name>`

A sub-agent has no pane and no hooks, and it runs in its parent's directory,
so it shares the parent's seat. Murail's A4 already says the parent routes
between its sub-agents; hail makes that route addressable.

- **Target `murail-2b/recip-consumer`:** the message goes to murail-2b's
  mailbox with `for: recip-consumer`. The envelope shows `for:recip-consumer`,
  and the parent's pane is woken as the kind decides. The parent forwards it
  through its agent handle.
- **`--as <name>`:** a sub-agent signs as `seat/name`. `from:` and `reply:`
  read `murail-2b/recip-consumer`, so the answer comes back through the parent.
- **Grammar:** `Addr::parse` splits on `/` first, then on `@`, and checks each
  part at that one boundary (hail-cr9). A part beginning with `%` is a pane:
  `seat/%N` resolves exactly like the target `%N`, which already picks a
  sub-seat in a shared directory, plus a check that the pane is in that seat.
  This means a `from:` value can be pasted as a target. Any other part is a
  sub-agent name, following the seat-name rules. No live seat name contains
  `/` or `@`.
- **Refusals:** an unknown target whose prefix is a known seat
  (`murail-2b-recip-consumer`) is refused with `not a seat; if it is a
  sub-agent of murail-2b, send to murail-2b/recip-consumer (its parent
  relays)`.
- **The skill rule:** a sub-agent shares its parent's directory and
  environment, so it runs no verb that reads mail (`inbox`, `deliver`,
  `brief`); those would take the parent's mail and prune its pending sends.
  It sends, with `--as`.

## 3. Holds lapse

There are 33 recorded holds today. 24 are older than a day, and the oldest
dates from 2026-09-05. Ferry's locks and `just land` now serialize landing,
installs and timing runs.

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
  lapsed hold finds it there, moves nothing, and says it had lapsed.
- **`gc`** deletes `holds/lapsed/` entries past the archive age, and lapsed
  holds whose issuer has no mailbox left (0.3 issuers such as `%2` or `worker`
  never run `brief` again).
- **Steering:** the skill's hold section opens with "a hold is a person's
  decision ('don't touch X while I redesign it'); to serialize landing,
  installs or timing runs, use the tool's lock (`just land`, `ferry hold`,
  the host lease)."

## What changes for Murail seats

- An `fyi` no longer interrupts a seat whose hooks run.
- `hail note` exists.
- `seat/name` reaches a sub-agent through its parent.
- Holds lapse after 8h, and blocks after 7d, unless given `--for`.
- The skill carries all of this, and seats pick it up at their next session
  start.

## Out of scope

- A digest or broadcast verb: a coordinator that wants a digest reads the bead.
- Sub-agents receiving mail directly: that would need an identity hail cannot
  verify.
- Detecting progress by its content.

## Release

0.4.1 shipped §0. 0.5.0 ships §1 to §3 in one CHANGELOG entry; §3's review added that a brief retires only as many lapsed holds as it shows (hail-lzg).
The spec, DESIGN.md, the skill, `src/help.rs` and the scenarios change
together. New scenarios:

- a quiet `fyi` types nothing and arrives with the next prompt;
- an `fyi` to a seat whose hooks have not run is typed;
- `note` posts, and fails with bd's error;
- `seat/name` delivers to the parent with `for:`, and `--as` signs;
- `seat/%N` resolves like `%N`;
- a hold lapses for every reader, and is retired by its issuer's brief;
- `release` of a lapsed hold, and the `gc` sweep.
