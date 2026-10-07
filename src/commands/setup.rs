//! `hail setup`: install the Claude Code and Codex hooks.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use similar::TextDiff;

use crate::error::{Error, Result};
use crate::hooks::setup::{merge_claude, merge_codex, wanted};

/// One harness config file hail installs its hooks into.
pub struct Harness {
    pub name: &'static str,
    pub file: PathBuf,
    merge: fn(&str) -> Result<String>,
    note: &'static str,
}

impl Harness {
    /// The harness is installed when its config directory exists.
    pub fn installed(&self) -> bool {
        self.file.parent().is_some_and(std::path::Path::is_dir)
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The configs to install into. Claude Code reads `$CLAUDE_CONFIG_DIR` when
/// set, else `~/.claude`; sessions on one machine can use either, so both
/// are covered when they differ.
pub fn harnesses() -> Vec<Harness> {
    let mut claude_dirs = vec![home().join(".claude")];
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)
        && !claude_dirs.contains(&d)
    {
        claude_dirs.push(d);
    }
    let codex_dir =
        std::env::var_os("CODEX_HOME").map_or_else(|| home().join(".codex"), PathBuf::from);
    let mut out: Vec<Harness> = claude_dirs
        .into_iter()
        .map(|d| Harness {
            name: "Claude Code",
            file: d.join("settings.json"),
            merge: merge_claude,
            note: "a new session picks it up",
        })
        .collect();
    out.push(Harness {
        name: "Codex",
        file: codex_dir.join("config.toml"),
        merge: merge_codex,
        note: "run /hooks in Codex once to trust the changed hooks; a new session picks them up",
    });
    out
}

/// What `merge` would change, or None when the file is current. A harness
/// whose config directory does not exist is not installed: skipped.
pub fn plan(h: &Harness) -> Result<Option<(String, String)>> {
    if !h.installed() {
        return Ok(None);
    }
    let old = fs::read_to_string(&h.file).unwrap_or_default();
    let new = (h.merge)(&old)?;
    Ok((old != new).then_some((old, new)))
}

pub fn setup(check: bool, yes: bool) -> Result<u8> {
    let mut drift = false;
    for h in harnesses() {
        let path = h.file.display().to_string();
        if !h.installed() {
            outln!(
                "skip  {}: {} not installed",
                h.name,
                h.file
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            continue;
        }
        let Some((old, new)) = plan(&h)? else {
            outln!("ok    {}: hail hooks current in {path}", h.name);
            continue;
        };
        drift = true;
        outln!(
            "{}",
            TextDiff::from_lines(&old, &new)
                .unified_diff()
                .header(&path, &path)
        );
        if check {
            outln!("drift {}: run hail setup", h.name);
            continue;
        }
        if !(yes || confirm(&format!("update {path}?"))?) {
            outln!("left  {}: unchanged", h.name);
            continue;
        }
        if !old.is_empty() {
            let backup = h.file.with_extension(format!(
                "{}.hail-backup",
                h.file.extension().and_then(|e| e.to_str()).unwrap_or("")
            ));
            fs::write(&backup, &old).map_err(Error::at(&backup))?;
        }
        crate::store::write_atomic(&h.file, new.as_bytes())?;
        outln!("done  {}: updated {path}; {}", h.name, h.note);
    }
    for (event, cmd) in wanted("<harness>") {
        if check {
            break;
        }
        outln!("      {event}: {cmd}");
    }
    Ok(u8::from(check && drift))
}

fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::Usage(format!(
            "{question} needs a yes: rerun with --yes, or run it in a terminal"
        )));
    }
    out!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| Error::State(e.to_string()))?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}
