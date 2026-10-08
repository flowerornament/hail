//! `hail note <bead> [headline]` (body on stdin): progress, for the record.
//! It goes on the bead and nowhere else: no mailbox, no pane, no receipt, so
//! it costs nobody a turn until someone reads the bead. bd runs in this
//! directory, so a jj workspace whose `.beads` redirects reaches the shared
//! database.

use crate::bd::{self, Posted};
use crate::commands::send::{Form, headline_and_body};
use crate::ctx::Ctx;
use crate::envelope;
use crate::error::{Error, Result};

pub fn run(ctx: &Ctx, bead: &str, headline: Option<String>) -> Result<u8> {
    if !envelope::is_bead_id(bead) {
        return Err(Error::Usage(format!(
            "{bead} is not a bead id; hail note <bead> '<headline>', e.g. hail note murail-ke7is 'gate green'"
        )));
    }
    let me = ctx.require_seat()?;
    let (raw, body) = headline_and_body(&Form::Current { headline })?;
    let headline = envelope::sanitize_headline(&raw);
    if headline.is_empty() {
        return Err(Error::Usage(
            "empty note: say what happened in one line, details on stdin".into(),
        ));
    }
    let text = match body {
        Some(b) => format!("[{}] {headline}\n\n{b}", me.name),
        None => format!("[{}] {headline}", me.name),
    };
    match bd::comment(bead, &text) {
        Posted::Comment(n) => outln!("bead={bead} comment={n}"),
        Posted::Unnumbered => outln!("bead={bead}"),
        // The bead is the only copy, so a failure is an error, not a warning.
        Posted::Failed(why) => {
            return Err(Error::State(format!(
                "the note was not saved: bd could not comment on {bead} ({why}); run bd show {bead} here to see why"
            )));
        }
    }
    Ok(0)
}
