//! Harness integration: what hooks print, and how `hail setup` installs them.

pub mod setup;

use std::io::{IsTerminal, Read};
use std::sync::mpsc;
use std::time::Duration;

/// The context a prompt hook adds, the same shape for Claude Code and Codex.
pub fn hook_json(event: &str, context: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": { "hookEventName": event, "additionalContext": context }
    })
    .to_string()
}

/// Read stdin only when its first byte arrives within `wait`, then to EOF.
/// An agent's tool runner often leaves stdin as an open pipe that never ends;
/// reading it unconditionally would hang a send forever.
pub fn read_stdin_if_ready(wait: Duration) -> Option<Vec<u8>> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (started_tx, started_rx) = mpsc::channel::<bool>();
    let (done_tx, done_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut lock = std::io::stdin().lock();
        let mut first = [0u8; 1];
        if let Ok(1) = lock.read(&mut first) {
            let _ = started_tx.send(true);
            let mut buf = vec![first[0]];
            let _ = lock.read_to_end(&mut buf);
            let _ = done_tx.send(buf);
        } else {
            let _ = started_tx.send(false);
        }
    });
    match started_rx.recv_timeout(wait) {
        // A writer is there: a heredoc or pipe always ends.
        Ok(true) => done_rx.recv().ok(),
        _ => None,
    }
}
