//! How an envelope reaches an agent: typed into the agent's pane. Another
//! way to wake an agent (a harness-native channel) belongs behind `Wake`.
//! Nothing outside this module types into a pane for a send.

pub mod agent;
pub mod dialog;
pub mod pane_map;
pub mod tmux;

use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use tmux::{Pane, Tmux};

/// Wait between the text showing in the composer and Enter. It exists for
/// paste-burst detection after the text has rendered, so faster verification
/// does not shorten it. Lower it only after a live trial in a Claude and a
/// Codex pane, under load, with no envelope left unsubmitted.
pub const SUBMIT_DELAY: Duration = Duration::from_millis(300);
const VERIFY_EVERY: Duration = Duration::from_millis(25);
/// How long to wait for the typed text to show. A busy agent may not read
/// its input for seconds (Codex mid-turn, or a loaded machine); pressing
/// Enter before it has read the text loses the submit (see `type_verified`),
/// so the wait is long, and costs time only while the pane is not reading.
const VERIFY_FOR: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub enum Woken {
    /// Typed, seen in the pane, and submitted unless the caller said not to.
    Typed,
    /// Typed once but not seen within the verify window; not submitted.
    NotConfirmed,
}

pub trait Wake {
    fn wake(&self, pane: &Pane, envelope: &str, submit: bool) -> Result<Woken>;
}

pub struct TypedEnvelope<'a> {
    pub tmux: &'a Tmux,
}

impl Wake for TypedEnvelope<'_> {
    fn wake(&self, pane: &Pane, envelope: &str, submit: bool) -> Result<Woken> {
        if type_verified(self.tmux, pane, envelope)? == Woken::NotConfirmed {
            return Ok(Woken::NotConfirmed);
        }
        if submit {
            sleep(SUBMIT_DELAY);
            // Leave copy mode again: a scroll in the last 300 ms would turn
            // Enter into a copy-mode command.
            let in_mode = self
                .tmux
                .run(&["display-message", "-t", &pane.id, "-p", "#{pane_in_mode}"])
                .is_ok_and(|s| s.trim() == "1");
            self.tmux.send_key(&pane.id, "Enter", in_mode)?;
        }
        Ok(Woken::Typed)
    }
}

/// Refuse a pane that shows a permission dialog: text typed over one is
/// discarded and Enter approves the command.
pub fn guard_dialog(tmux: &Tmux, pane: &Pane) -> Result<()> {
    let screen = tmux.capture(&pane.id, None)?;
    if dialog::shows_dialog(&screen) {
        return Err(Error::Dialog(format!(
            "{} shows a permission/approval dialog; text typed there is discarded and Enter approves it. \
             Read it (hail read {} 10), resolve it, then resend; --force only after reading",
            pane.id, pane.id
        )));
    }
    Ok(())
}

/// Type text and poll until it shows in the pane. Never clears or retypes:
/// retyping doubled messages and ate drafts.
///
/// Seeing the text proves the agent has read it, so Enter, sent later, is
/// read later. That matters for Codex: keys it reads in one batch count as a
/// paste, and an Enter inside a paste is a newline, so the envelope would sit
/// in the composer. The probe must therefore be text only this send put on
/// the screen: an envelope's `id:` token. The opening characters
/// (`[hail kind:fyi from:… reply:…`) repeat in every envelope from one seat,
/// and an earlier one still on screen matched at once (hail-lha).
pub fn type_verified(tmux: &Tmux, pane: &Pane, text: &str) -> Result<Woken> {
    let probe = probe(text);
    tmux.type_text(&pane.id, text, pane.in_mode)?;
    if probe.is_empty() {
        return Ok(Woken::Typed);
    }
    let start = Instant::now();
    while start.elapsed() < VERIFY_FOR {
        sleep(VERIFY_EVERY);
        let screen = tmux.capture(&pane.id, None).unwrap_or_default();
        let flat: String = screen.chars().filter(|c| !c.is_whitespace()).collect();
        if flat.contains(&probe) {
            return Ok(Woken::Typed);
        }
    }
    Ok(Woken::NotConfirmed)
}

/// What to look for on screen: an envelope's `id:<id>` token, unique to this
/// message; for any other text (`hail type`), its first 40 non-space
/// characters, even when it happens to contain an `id:` word.
fn probe(text: &str) -> String {
    let id = text
        .starts_with("[hail ")
        .then(|| {
            text.split_whitespace()
                .find(|w| w.starts_with("id:") && w.len() > 3)
        })
        .flatten()
        .map(|w| w.trim_end_matches(']'));
    match id {
        Some(id) => id.to_string(),
        None => text
            .chars()
            .filter(|c| !c.is_whitespace())
            .take(40)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::probe;

    #[test]
    fn an_envelope_is_found_by_its_id() {
        let a = "[hail kind:fyi from:murail-2b/%8 reply:murail-2b id:1007T224623-3b07 bead:x] one";
        let b = "[hail kind:fyi from:murail-2b/%8 reply:murail-2b id:1007T225740-cdf8] two";
        assert_eq!(probe(a), "id:1007T224623-3b07");
        assert_eq!(probe(b), "id:1007T225740-cdf8");
        // The old probe, the first 40 non-space characters, was the same for both.
        let old = |s: &str| {
            s.chars()
                .filter(|c| !c.is_whitespace())
                .take(40)
                .collect::<String>()
        };
        assert_eq!(old(a), old(b));
        assert_eq!(probe("y"), "y");
        // Not an envelope: typed user text keeps the first-40 probe, even with an id: word.
        assert_eq!(
            probe("see id:1007T224623-3b07 for details"),
            "seeid:1007T224623-3b07fordetails"
        );
    }
}
