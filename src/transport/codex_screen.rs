//! Where a typed envelope sits on a Codex screen after Enter. Codex does not
//! always submit what Enter submits: input that misses the current turn waits
//! in sections below the transcript, and some of it is sent only when no turn
//! is running, which under a goal that keeps starting turns can be a long
//! time. Pure: screen text in, a verdict out.

// The section headers below are from Codex 0.162.0
// (`tui/src/bottom_pane/pending_input_preview.rs`). Recheck them when Codex
// changes: a header that no longer matches reads as "submitted".

/// Sent at the agent's next tool call: delivered, just not yet.
pub const STEER: &str = "• Messages to be submitted after next tool call";
/// A steer Codex could not deliver, retried when the turn ends.
pub const END_OF_TURN: &str = "• Messages to be submitted at end of turn";
/// Sent only when no turn is running.
pub const QUEUED: &str = "• Queued follow-up inputs";
/// Footer while a goal runs: turns follow each other, so "end of turn" and
/// "queued" input can wait until the goal pauses.
pub const GOAL: &str = "Pursuing goal";

/// Where the envelope's id was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// In the transcript, or no longer on screen: the agent has it.
    Submitted,
    /// Still in the composer: Enter did not take.
    Composer,
    /// Held as a steer until the next tool call.
    Steer,
    /// Held until the turn ends.
    EndOfTurn,
    /// Held until no turn is running.
    Queued,
}

/// Classify by the nearest marker above the last line holding `id`: a held
/// section's header, or a `› ` line, which starts both a transcript message
/// and the composer. The composer is the last `› ` block, with no `• ` line
/// (agent output, a status line, a held section) after it.
pub fn classify(screen: &str, id: &str) -> Landed {
    let lines: Vec<&str> = screen.lines().collect();
    let Some(at) = lines.iter().rposition(|l| l.contains(id)) else {
        return Landed::Submitted;
    };
    for (i, line) in lines[..=at].iter().enumerate().rev() {
        if line.starts_with(STEER) {
            return Landed::Steer;
        }
        if line.starts_with(END_OF_TURN) {
            return Landed::EndOfTurn;
        }
        if line.starts_with(QUEUED) {
            return Landed::Queued;
        }
        if line.starts_with("› ") {
            let later = &lines[i + 1..];
            let is_composer = !later
                .iter()
                .any(|l| l.starts_with("› ") || l.starts_with("• "));
            return if is_composer {
                Landed::Composer
            } else {
                Landed::Submitted
            };
        }
    }
    Landed::Submitted
}

/// Whether the screen shows a goal being pursued.
pub fn pursuing_goal(screen: &str) -> bool {
    screen.contains(GOAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "id:1011T055221-e0a3";
    const FOOTER: &str =
        "  gpt low · ~/code/api-1b · Pursuing goal (4m)\n  ← for agents · ? for shortcuts\n";

    fn screen(body: &str) -> String {
        format!("• earlier output\n{body}{FOOTER}")
    }

    #[test]
    fn sent_and_answered() {
        let s = screen(&format!(
            "› [hail kind:ruling from:api-1a reply:api-1a {ID}] go\n• Working (3s • esc to interrupt)\n› Ask Codex to do anything\n"
        ));
        assert_eq!(classify(&s, ID), Landed::Submitted);
    }

    #[test]
    fn left_in_the_composer_even_when_wrapped() {
        let s = screen(&format!(
            "› [hail kind:ruling from:api-1a reply:api-1a\n  {ID}] a headline long enough to wrap\n"
        ));
        assert_eq!(classify(&s, ID), Landed::Composer);
    }

    #[test]
    fn held_sections() {
        for (header, want) in [
            (STEER, Landed::Steer),
            (END_OF_TURN, Landed::EndOfTurn),
            (QUEUED, Landed::Queued),
        ] {
            let s = screen(&format!(
                "• Working (9s • esc to interrupt)\n{header}\n  ↳ [hail kind:ruling from:api-1a reply:api-1a {ID}] go\n› Ask Codex to do anything\n"
            ));
            assert_eq!(classify(&s, ID), want, "{header}");
        }
    }

    #[test]
    fn an_older_copy_in_the_transcript_does_not_hide_a_held_one() {
        let s = screen(&format!(
            "› [hail kind:ruling from:api-1a reply:api-1a {ID}] go\n• ok\n{QUEUED}\n  ↳ [hail kind:ruling from:api-1a reply:api-1a {ID}] go\n› Ask Codex to do anything\n"
        ));
        assert_eq!(classify(&s, ID), Landed::Queued);
    }

    #[test]
    fn gone_from_the_screen_is_submitted() {
        assert_eq!(
            classify(&screen("› Ask Codex to do anything\n"), ID),
            Landed::Submitted
        );
        assert!(pursuing_goal(FOOTER));
    }
}
