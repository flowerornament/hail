//! A permission dialog is recognised by its own question and option lines,
//! never by a bare word: Codex prints "Approved" in its history and status, and
//! matching it refused idle panes until agents learned to --force.

/// Each entry is a literal that appears only while a dialog is open.
pub const PATTERNS: &[(&str, &str)] = &[
    // Claude Code (2.x permission prompt)
    ("claude", "Do you want to proceed"),
    ("claude", "Yes, and don't ask again"),
    ("claude", "Esc to cancel"),
    ("claude", "No, and tell Claude what to do"),
    // Codex 0.160 approval overlay
    ("codex", "Would you like to run the following command?"),
    ("codex", "Would you like to make the following edits?"),
    ("codex", "Would you like to grant these permissions?"),
    ("codex", "Would you like to proceed"),
    ("codex", "Yes, proceed"),
    ("codex", "Yes, just this once"),
    ("codex", "No, and tell Codex what to do"),
    // Generic prompts
    ("any", "(y/n)"),
    ("any", "Allow once"),
];

/// True when the last eight non-blank lines of a pane show a dialog.
pub fn shows_dialog(screen: &str) -> bool {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = &lines[lines.len().saturating_sub(8)..];
    tail.iter()
        .any(|l| PATTERNS.iter().any(|(_, p)| l.contains(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_permission_prompt() {
        let s = "Bash(rm -rf build)\nDo you want to proceed?\n  1. Yes\n  2. Yes, and don't ask again\n  3. No\nEsc to cancel\n";
        assert!(shows_dialog(s));
    }

    #[test]
    fn codex_idle_after_approved_is_not_a_dialog() {
        let s = "✔ Approved command: just land\n• Ran just land\n  └ ok\n⚠ 4 warnings · f2 to view\n› \n";
        assert!(!shows_dialog(s));
    }

    #[test]
    fn codex_approval_overlay() {
        let s = "Would you like to run the following command?\n  $ rm -rf build\n› 1. Yes, proceed (y)\n";
        assert!(shows_dialog(s));
    }

    #[test]
    fn only_the_last_eight_lines_count() {
        let s: String = std::iter::once("Do you want to proceed?\n".to_string())
            .chain((0..8).map(|i| format!("output {i}\n")))
            .collect();
        assert!(!shows_dialog(&s));
    }
}
