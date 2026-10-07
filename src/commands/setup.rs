//! `hail setup`: install the hooks; `hail doctor`: say what is wrong and how
//! to fix it, one line each.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use similar::TextDiff;

use crate::ctx::{Ctx, PaneMap};
use crate::error::{Error, Result};
use crate::hooks::setup::{merge_claude, merge_codex, wanted};
use crate::store::list_names;
use crate::transport::tmux::Tmux;

struct Harness {
    name: &'static str,
    file: PathBuf,
    merge: fn(&str) -> Result<String>,
    note: &'static str,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The configs to install into. Claude Code reads `$CLAUDE_CONFIG_DIR` when
/// set, else `~/.claude`; sessions on one machine can use either, so both
/// are covered when they differ.
fn harnesses() -> Vec<Harness> {
    let mut claude_dirs = vec![home().join(".claude")];
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from) {
        if !claude_dirs.contains(&d) {
            claude_dirs.push(d);
        }
    }
    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"));
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
fn plan(h: &Harness) -> Result<Option<(String, String)>> {
    if !h.file.parent().is_some_and(|d| d.is_dir()) {
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
        if !h.file.parent().is_some_and(|d| d.is_dir()) {
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
            fs::write(&backup, &old).map_err(|e| Error::io(&backup, e))?;
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
    Ok(if check && drift { 1 } else { 0 })
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

pub fn doctor(ctx: &Ctx) -> Result<u8> {
    let mut problems = 0;
    let mut say = |ok: bool, line: String| {
        if !ok {
            problems += 1;
        }
        outln!("{} {line}", if ok { "ok  " } else { "FIX " });
    };
    say(
        true,
        format!(
            "hail {} — state {}",
            env!("CARGO_PKG_VERSION"),
            ctx.store.root().display()
        ),
    );
    if ctx.store.legacy_present() {
        say(false, "0.3 state is not migrated: run hail migrate".into());
    }

    let seat = ctx.seat_here();
    match &seat {
        Ok(Some(s)) => match ctx.bind(s) {
            Ok(()) => say(
                true,
                format!(
                    "seat here: {} ({}, {})",
                    s.name,
                    s.source.describe(),
                    s.root.display()
                ),
            ),
            Err(e) => say(false, e.to_string()),
        },
        Ok(None) => say(
            true,
            format!(
                "no seat here ({}); fine outside a workspace",
                ctx.cwd.display()
            ),
        ),
        Err(e) => say(false, e.to_string()),
    }

    match Tmux::detect() {
        Err(e) => say(false, e.to_string()),
        Ok(t) => match PaneMap::load(&t, ctx.home.as_deref()) {
            Err(e) => say(
                false,
                format!("tmux ({}): {e}; start tmux or set HAIL_SOCKET", t.source),
            ),
            Ok(pm) => {
                say(
                    true,
                    format!(
                        "tmux ({}): {} panes, {} agents",
                        t.source,
                        pm.panes.len(),
                        pm.panes.iter().filter(|p| p.agent.is_some()).count()
                    ),
                );
                if let Ok(Some(s)) = &seat {
                    let agents = pm.agents_in(&s.name);
                    let ids: Vec<&str> = agents.iter().map(|p| p.id.as_str()).collect();
                    match agents.len() {
                        0 => say(
                            true,
                            format!(
                                "no agent pane in seat {} (mail waits for the next session)",
                                s.name
                            ),
                        ),
                        1 => say(true, format!("wake pane for {}: {}", s.name, ids[0])),
                        n => say(
                            true,
                            format!(
                                "seat {} is shared by {n} agents ({}): each Claude pane is addressed as {}@<pane>",
                                s.name,
                                ids.join(" "),
                                s.name
                            ),
                        ),
                    }
                }
                // Unread mail nobody will pick up: a sub-seat whose pane is
                // gone, or a seat no pane sits in (an old label, an agent
                // that is not running). The second is normal for an agent
                // that is off; it is listed so nothing waits unseen.
                let mut waiting = Vec::new();
                for name in ctx.store.seat_names() {
                    let unread = ctx.store.mailbox(&name).unread().len();
                    if unread == 0 {
                        continue;
                    }
                    match crate::seat::split_sub_seat(&name) {
                        (_, Some(p)) if pm.find(p).is_none() => say(
                            false,
                            format!(
                                "orphaned sub-seat {name} holds {unread} unread (its pane is gone): hail show <id> reads them"
                            ),
                        ),
                        (base, None) if pm.in_seat(base).is_empty() => {
                            waiting.push(format!("{name} ({unread})"))
                        }
                        _ => {}
                    }
                }
                if !waiting.is_empty() {
                    outln!(
                        "note unread mail waits in seats with no pane now: {}",
                        waiting.join(", ")
                    );
                }
            }
        },
    }

    for h in harnesses() {
        match plan(&h) {
            Ok(None) if h.file.parent().is_some_and(|d| d.is_dir()) => say(
                true,
                format!("{} hooks current ({})", h.name, h.file.display()),
            ),
            Ok(None) => {}
            Ok(Some(_)) => say(
                false,
                format!(
                    "{} hooks missing or outdated in {}: run hail setup",
                    h.name,
                    h.file.display()
                ),
            ),
            Err(e) => say(false, e.to_string()),
        }
    }

    for name in ctx.store.seat_names() {
        let n = list_names(&ctx.store.seat_dir(&name).join("cur")).len();
        if n > 5000 {
            say(
                false,
                format!("seat {name} keeps {n} read messages: run hail gc"),
            );
        }
    }
    Ok(if problems > 0 { 1 } else { 0 })
}
