//! Harness integration: what hooks print, and how `hail setup` installs them.

pub mod setup;

/// The context a prompt hook adds, the same shape for Claude Code and Codex.
pub fn hook_json(event: &str, context: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": { "hookEventName": event, "additionalContext": context }
    })
    .to_string()
}
