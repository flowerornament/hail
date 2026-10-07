//! The tmux client: socket detection and the handful of commands hail uses.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use super::agent::{self, Agent};
use crate::error::{Error, Result};

/// Which server to talk to: `HAIL_SOCKET` (or `TMUX_BRIDGE_SOCKET`), else the
/// socket in a live `$TMUX`, else the default server.
#[derive(Debug, Clone)]
pub struct Tmux {
    socket: Option<PathBuf>,
    pub source: &'static str,
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: String,
    pub pid: u32,
    pub path: PathBuf,
    pub command: String,
    pub in_mode: bool,
    pub session: String,
    pub window: String,
    pub size: String,
    /// The agent this pane runs, when it runs one.
    pub agent: Option<Agent>,
}

impl Tmux {
    pub fn detect() -> Result<Self> {
        for var in ["HAIL_SOCKET", "TMUX_BRIDGE_SOCKET"] {
            if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                let p = PathBuf::from(&v);
                if !is_socket(&p) {
                    return Err(Error::Usage(format!(
                        "{var}={} is not a tmux socket; unset it or point it at a live server",
                        p.display()
                    )));
                }
                return Ok(Self {
                    socket: Some(p),
                    source: if var == "HAIL_SOCKET" {
                        "HAIL_SOCKET"
                    } else {
                        "TMUX_BRIDGE_SOCKET"
                    },
                });
            }
        }
        if let Some(t) = std::env::var_os("TMUX") {
            let s = t.to_string_lossy();
            let p = PathBuf::from(s.split(',').next().unwrap_or(""));
            if is_socket(&p) {
                return Ok(Self {
                    socket: Some(p),
                    source: "$TMUX",
                });
            }
        }
        Ok(Self {
            socket: None,
            source: "default server",
        })
    }

    fn command(&self) -> Command {
        let mut c = Command::new("tmux");
        if let Some(s) = &self.socket {
            c.arg("-S").arg(s);
        }
        c
    }

    /// Run one tmux command line (several when joined by a lone ";").
    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = self.command().args(args).output().map_err(|e| {
            Error::State(format!(
                "cannot run tmux: {e}; is it installed and on PATH?"
            ))
        })?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(Error::State(format!("tmux: {err}")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Every pane on the server in one call, with agents detected.
    pub fn panes(&self) -> Result<Vec<Pane>> {
        let fmt = "#{pane_id}\t#{pane_pid}\t#{pane_current_command}\t#{pane_in_mode}\t#{session_name}\t#{window_index}\t#{pane_width}x#{pane_height}\t#{pane_current_path}";
        let out = self.run(&["list-panes", "-a", "-F", fmt])?;
        let mut panes: Vec<Pane> = out
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.splitn(8, '\t').collect();
                if f.len() < 8 {
                    return None;
                }
                Some(Pane {
                    id: f[0].to_string(),
                    pid: f[1].parse().unwrap_or(0),
                    command: f[2].to_string(),
                    in_mode: f[3] == "1",
                    session: f[4].to_string(),
                    window: f[5].to_string(),
                    size: f[6].to_string(),
                    path: PathBuf::from(f[7]),
                    agent: None,
                })
            })
            .collect();
        agent::detect(&mut panes);
        Ok(panes)
    }

    /// The pane id for any tmux target (`%5`, `sess:1.2`, `3`).
    pub fn pane_id(&self, target: &str) -> Result<String> {
        self.run(&["display-message", "-t", target, "-p", "#{pane_id}"])
            .map(|s| s.trim().to_string())
            .map_err(|_| {
                Error::Usage(format!(
                    "no pane {target}; hail seats lists seats and their panes"
                ))
            })
    }

    pub fn capture(&self, pane: &str, start: Option<i64>) -> Result<String> {
        let mut args = vec![
            "capture-pane".to_string(),
            "-t".into(),
            pane.into(),
            "-p".into(),
            "-J".into(),
        ];
        if let Some(s) = start {
            args.push("-S".into());
            args.push(s.to_string());
        }
        self.run(&args)
    }

    /// Leave copy mode (a mouse-wheel scroll starts it; typed keys would run
    /// as mode commands, and vi's ':' opens a prompt) and type the text
    /// literally, in one tmux call.
    pub fn type_text(&self, pane: &str, text: &str, leave_mode: bool) -> Result<()> {
        let mut args: Vec<&str> = Vec::new();
        if leave_mode {
            args.extend(["send-keys", "-t", pane, "-X", "cancel", ";"]);
        }
        // tmux ends a command at an argument ending in ';' unless it is
        // escaped as "\;", so a headline ending in ';' needs the escape.
        let escaped;
        let text = match text.strip_suffix(';') {
            Some(stem) => {
                escaped = format!("{stem}\\;");
                escaped.as_str()
            }
            None => text,
        };
        args.extend(["send-keys", "-t", pane, "-l", "--", text]);
        self.run(&args).map(|_| ())
    }

    pub fn send_key(&self, pane: &str, key: &str, leave_mode: bool) -> Result<()> {
        let mut args: Vec<&str> = Vec::new();
        if leave_mode {
            args.extend(["send-keys", "-t", pane, "-X", "cancel", ";"]);
        }
        args.extend(["send-keys", "-t", pane, key]);
        self.run(&args).map(|_| ())
    }
}

fn is_socket(p: &std::path::Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(p).is_ok_and(|m| m.file_type().is_socket())
}
