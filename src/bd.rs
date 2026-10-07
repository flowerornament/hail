//! Post a message body to a beads issue, opportunistically. Runs after the
//! envelope is submitted, so a slow bd never delays the wake.

use std::io::Write;
use std::process::{Command, Stdio};

/// `Some(Some(n))`: posted as comment n. `Some(None)`: posted, number unknown.
/// `None`: bd is missing or failed.
pub fn comment(bead: &str, body: &str) -> Option<Option<u64>> {
    let mut path = std::env::temp_dir();
    path.push(format!("hail-body-{}-{bead}.md", std::process::id()));
    let mut f = std::fs::File::create(&path).ok()?;
    f.write_all(body.as_bytes()).ok()?;
    f.write_all(b"\n").ok()?;
    drop(f);
    let out = Command::new("bd")
        .args(["comment", bead, "--file"])
        .arg(&path)
        .arg("--json")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&path);
    let out = out.ok()?;
    if !out.status.success() {
        return None;
    }
    let n = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()
        .and_then(|v| {
            let id = v.get("id")?;
            id.as_u64().or_else(|| id.as_str()?.parse().ok())
        });
    Some(n)
}
