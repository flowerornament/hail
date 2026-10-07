//! What the hooks do. hail installs two hooks into each harness (Claude Code,
//! Codex), and both are ordinary verbs:
//!
//! - `SessionStart` runs `hail brief --hook` (`commands::brief`): the standing
//!   state as plain text, which the harness adds to the context. It prints
//!   nothing outside a seat.
//! - `UserPromptSubmit` runs `hail deliver --format claude|codex`
//!   (`commands::receive::deliver`): unread bodies are claimed and injected
//!   as [`hook_json`], and the claim is the receipt.
//!
//! `hail setup` (`commands::setup`) writes them into the harness config, with
//! the merge itself in `setup`, which is pure. A hook runs on every prompt of
//! every session, so it must never fail one: both verbs exit 0 and report
//! errors through [`log_error`].

pub mod setup;

use std::fs;
use std::io::Write;

use crate::error::Error;
use crate::policy::HOOK_LOG_MAX;
use crate::store::Store;
use crate::time;

/// The context a prompt hook adds, the same shape for Claude Code and Codex.
pub fn hook_json(event: &str, context: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": { "hookEventName": event, "additionalContext": context }
    })
    .to_string()
}

/// Append one line to `hook-errors.log`, rotating it past [`HOOK_LOG_MAX`].
pub fn log_error(store: &Store, verb: &str, e: &Error) {
    let path = store.root().join("hook-errors.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() > HOOK_LOG_MAX) {
        let _ = fs::rename(&path, path.with_extension("log.1"));
    }
    let _ = fs::create_dir_all(store.root());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {verb}: {e}", time::iso(time::now()));
    }
}

/// A claim is only worth making when the output can reach the agent. Rust
/// reopens a closed fd 1 as /dev/null at startup, and writes there always
/// succeed, so a hook killed or redirected to /dev/null would lose its mail.
pub fn stdout_reaches_anyone() -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata("/dev/fd/1"), fs::metadata("/dev/null")) {
        (Err(_), _) => false,
        (Ok(out), Ok(null)) => out.rdev() != null.rdev() || out.ino() != null.ino(),
        (Ok(_), Err(_)) => true,
    }
}
