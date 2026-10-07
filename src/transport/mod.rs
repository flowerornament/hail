//! How an envelope reaches an agent. 0.4 types it into the agent's pane; S3
//! (0.5) adds a typed wake token and harness-native channels behind `Wake`.
//! Nothing outside this module types into a pane for a send.

pub mod dialog;
pub mod panes;
pub mod tmux;

use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use tmux::{Pane, Tmux};

/// Wait between the text showing in the composer and Enter. It exists for
/// paste-burst detection after the text has rendered, so faster verification
/// does not shorten it. Lower it only after the trial in spec §7.
pub const SUBMIT_DELAY: Duration = Duration::from_millis(300);
const VERIFY_EVERY: Duration = Duration::from_millis(25);
const VERIFY_FOR: Duration = Duration::from_secs(2);

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

/// Type text and poll until its opening characters show in the pane. Never
/// clears or retypes: retyping doubled messages and ate drafts (0.3.6).
pub fn type_verified(tmux: &Tmux, pane: &Pane, text: &str) -> Result<Woken> {
    let probe: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(40)
        .collect();
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
