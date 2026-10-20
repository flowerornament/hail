# Five kinds

Status: implemented in 0.7.

hail had ten kinds: `ruling go nogo ask fyi stop hold block release
announce`. 0.7 keeps five: `ask fyi announce hold release`. The kind now
says one thing, whether the sender wants a reply; `--re` says what a message
answers.

## Why

A study of a working message store, across several weeks of agents
coordinating through hail, showed:

- **`ruling`, `go` and `nogo` were answers, not questions.** Most of them
  carried `--re`, nearly always to an `ask`. A reviewer's GO is the reply to
  "ready for review?"; it was sent as its own kind because there was one.
- **hail already treated `ruling`, `go` and `ask` the same.** All three are
  typed, carry the body hint, and ask for a reply. Since 0.6 they write no
  record. The kind changed nothing a recipient's tooling could see.
- **Agents put the verdict in the headline anyway.** Many `go` headlines
  opened with "GO", and verdict words crossed kinds: APPROVED appeared in
  rulings and gos, HOLD in nogos. The headline already carried the
  distinction the kind was meant to.
- **`stop` and `block` were rare,** and most `stop`s were retractions that
  an `announce` says as well. A `block` was a hold that lasted longer.

Ten kinds made every sender choose among near-synonyms, and every reader
learn which choice meant what. Two questions decide delivery: does the
sender wait for an answer, and must the recipient act now?

## The kinds

| kind | means | delivery |
|---|---|---|
| `ask` | I need a reply. A verdict leads the headline: `GO: land it, send the sha`. | Typed, with the body hint. |
| `fyi` | No reply needed. | Quiet where the recipient's hooks run. Typed when its `--re` names an `ask` the sender received (the asker is waiting), or where no hook runs. |
| `announce` | Act on this now. | Typed in full, no body. |
| `hold` | A person's decision in effect: don't touch X. `--for` up to 7d, 8h by default. | Typed in full, recorded until `release` or it lapses. |
| `release` | Lifts a hold: `--re <id>`. | Typed in full. |

An answer is any message with `--re`: an `fyi` for an outcome, an `ask` when
the answer needs one back (a review that wants a fix confirmed).

## Mapping

| was | now |
|---|---|
| `ruling` | `ask`, `RULING: …` first in the headline |
| `go` | `ask`, `GO: …` (or `fyi --re` when nothing more is wanted) |
| `nogo` | `ask`, `NOGO: …` |
| `stop` | `announce` |
| `block` | `hold --for <span>` |
| `done` (gone in 0.6) | `fyi --re <id>` |

## Compatibility

- **Sending** a removed kind fails (exit 1) with a usage error that names
  its replacement: `go is gone in 0.7: send ask, the verdict first in the
  headline ('GO: land it, send the sha')`. Nothing is written.
- **Reading** stays lenient, as for `done` in 0.6. A stored message of a
  removed kind is listed by `brief` and printed by `inbox` and `show` as it
  was written; `deliver` emits it as stored, since its kind is not one this
  version renders.
- **Rules that read stored mail** map a removed kind to its replacement:
  an `fyi --re` to a stored `go` or `ruling` is typed, as it was in 0.6; a
  stored `block` record is a hold, and one without `expires:` lapses after
  the hold default.
- **Tools that parse kinds** (a supervisor matching decision kinds, say)
  need the new list: decision kinds become `ask`.
