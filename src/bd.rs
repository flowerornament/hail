//! Post a message body to a beads issue, opportunistically. Runs after the
//! envelope is submitted, so a slow bd never delays the wake.

use std::io::Write;
use std::process::{Command, Stdio};

pub enum Posted {
    /// Posted as this comment number.
    Comment(u64),
    /// Posted; bd did not say which comment.
    Unnumbered,
    /// bd is missing or failed: the message is file-only.
    Failed,
}

pub fn comment(bead: &str, body: &str) -> Posted {
    let mut path = std::env::temp_dir();
    path.push(format!("hail-body-{}-{bead}.md", std::process::id()));
    let written = std::fs::File::create(&path).and_then(|mut f| writeln!(f, "{body}"));
    if written.is_err() {
        return Posted::Failed;
    }
    let out = Command::new("bd")
        .args(["comment", bead, "--file"])
        .arg(&path)
        .arg("--json")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&path);
    match out {
        Ok(o) if o.status.success() => serde_json::from_slice::<serde_json::Value>(&o.stdout)
            .ok()
            .and_then(|v| {
                let id = v.get("id")?;
                id.as_u64().or_else(|| id.as_str()?.parse().ok())
            })
            .map_or(Posted::Unnumbered, Posted::Comment),
        _ => Posted::Failed,
    }
}
