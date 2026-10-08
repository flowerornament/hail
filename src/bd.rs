//! Post a message body to a beads issue, opportunistically. Runs after the
//! envelope is submitted, so a slow bd never delays the wake.

use std::io::Write;
use std::process::{Command, Stdio};

pub enum Posted {
    /// Posted as this comment number.
    Comment(u64),
    /// Posted; bd did not say which comment.
    Unnumbered,
    /// bd is missing or failed, and why.
    Failed(String),
}

pub fn comment(bead: &str, body: &str) -> Posted {
    let mut path = std::env::temp_dir();
    path.push(format!("hail-body-{}-{bead}.md", std::process::id()));
    let written = std::fs::File::create(&path).and_then(|mut f| writeln!(f, "{body}"));
    if let Err(e) = written {
        return Posted::Failed(format!("writing {}: {e}", path.display()));
    }
    let out = Command::new("bd")
        .args(["comment", bead, "--file"])
        .arg(&path)
        .arg("--json")
        .stdin(Stdio::null())
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
        Ok(o) => Posted::Failed(reason(&String::from_utf8_lossy(&o.stderr))),
        Err(e) => Posted::Failed(format!("bd: {e}")),
    }
}

/// The line of bd's stderr that says what went wrong: its `Error:` line, else
/// its first line. bd wraps long errors, so the last line is often a fragment.
fn reason(stderr: &str) -> String {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    lines()
        .find(|l| l.starts_with("Error"))
        .or_else(|| lines().next())
        .unwrap_or("bd failed")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::reason;

    #[test]
    fn the_reason_is_the_error_line_not_the_wrapped_tail() {
        let wrapped = "Error: no beads database found\nHint: run 'bd where' to inspect\n      or set BEADS_DIR to point to your .beads directory\n";
        assert_eq!(reason(wrapped), "Error: no beads database found");
        assert_eq!(reason("\nsomething broke\nmore\n"), "something broke");
        assert_eq!(reason(""), "bd failed");
    }
}
