//! The tmux client: socket detection and the handful of commands hail uses.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

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
    /// The agent this pane runs, when it runs one (`claude`, `codex`).
    pub agent: Option<String>,
}

impl Tmux {
    pub fn detect() -> Result<Tmux> {
        for var in ["HAIL_SOCKET", "TMUX_BRIDGE_SOCKET"] {
            if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                let p = PathBuf::from(&v);
                if !is_socket(&p) {
                    return Err(Error::Usage(format!(
                        "{var}={} is not a tmux socket; unset it or point it at a live server",
                        p.display()
                    )));
                }
                return Ok(Tmux {
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
                return Ok(Tmux {
                    socket: Some(p),
                    source: "$TMUX",
                });
            }
        }
        Ok(Tmux {
            socket: None,
            source: "default server",
        })
    }

    pub fn socket(&self) -> Option<&PathBuf> {
        self.socket.as_ref()
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

    pub fn reachable(&self) -> bool {
        self.run(&["list-sessions"]).is_ok()
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
        detect_agents(&mut panes);
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
    std::fs::metadata(p)
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false)
}

/// Commands that count as agents. `HAIL_AGENT_COMMANDS` overrides the
/// default (the scenario harness drives `bash` and `cat` panes as agents).
pub fn agent_commands() -> Vec<String> {
    std::env::var("HAIL_AGENT_COMMANDS")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "claude,codex".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A pane runs an agent when its foreground command is one, or when its root
/// shell has an agent as a direct child: tmux reports the process an agent's
/// tool is running as the foreground command, so the shell check is needed
/// mid-turn. One `ps`, only when some pane is not already settled.
fn detect_agents(panes: &mut [Pane]) {
    let agents = agent_commands();
    if panes.iter().all(|p| agents.contains(&p.command)) {
        mark_agents(panes, &agents, &Procs::default());
        return;
    }
    let ps = Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "comm="])
        .output();
    let procs = ps
        .map(|o| Procs::parse(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    mark_agents(panes, &agents, &procs);
}

/// The process table, by command name: each process's own name and each
/// parent's children. Names are bare: macOS prints full paths for some
/// commands and `-zsh` for a login shell.
#[derive(Debug, Default)]
pub struct Procs {
    pub name: HashMap<u32, String>,
    pub children: HashMap<u32, Vec<String>>,
}

impl Procs {
    /// From `ps -A -o pid= -o ppid= -o comm=`.
    pub fn parse(ps: &str) -> Procs {
        let mut procs = Procs::default();
        for line in ps.lines() {
            let mut f = line.split_whitespace();
            let (Some(pid), Some(ppid)) = (f.next(), f.next()) else {
                continue;
            };
            let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else {
                continue;
            };
            let comm: Vec<&str> = f.collect();
            let comm = comm.join(" ");
            let name = comm
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_start_matches('-')
                .to_string();
            procs.children.entry(ppid).or_default().push(name.clone());
            procs.name.insert(pid, name);
        }
        procs
    }
}

/// A pane runs an agent when tmux names its foreground command as one, when
/// its root process is one, or when its root has an agent as a direct child
/// (a shell that started the agent; tmux then reports the agent's running
/// tool as the foreground command).
pub fn mark_agents(panes: &mut [Pane], agents: &[String], procs: &Procs) {
    let is_agent = |n: &String| agents.contains(n);
    for p in panes.iter_mut() {
        p.agent = if is_agent(&p.command) {
            Some(p.command.clone())
        } else if let Some(n) = procs.name.get(&p.pid).filter(|n| is_agent(n)) {
            Some(n.clone())
        } else {
            procs
                .children
                .get(&p.pid)
                .and_then(|kids| kids.iter().find(|k| is_agent(k)).cloned())
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, pid: u32, command: &str) -> Pane {
        Pane {
            id: id.into(),
            pid,
            path: PathBuf::from("/x"),
            command: command.into(),
            in_mode: false,
            session: "s".into(),
            window: "0".into(),
            size: "80x24".into(),
            agent: None,
        }
    }

    #[test]
    fn agents_by_foreground_root_or_child_of_the_root_shell() {
        // "<pid> <ppid> <comm>", in shapes seen on macOS: a login shell is
        // "-zsh", Codex is printed with its full path.
        let ps = "\
 3458     1 -zsh
 9001  3458 claude
 3484     1 -zsh
 9002  3484 /opt/homebrew/Caskroom/codex/0.160.1/bin/codex
 3600     1 /tmp/agents/claude
 3540     1 -zsh
 9003  3540 just
";
        let procs = Procs::parse(ps);
        let agents = vec!["claude".to_string(), "codex".to_string()];
        let mut panes = vec![
            pane("%1", 3458, "just"),   // Claude mid-tool: foreground is the tool
            pane("%2", 3484, "zsh"),    // Codex under a shell
            pane("%3", 3500, "claude"), // tmux names the agent
            pane("%4", 3600, "cat"),    // tmux names the binary; ps the agent
            pane("%5", 3540, "zsh"),    // a plain shell running just
        ];
        mark_agents(&mut panes, &agents, &procs);
        let got: Vec<Option<&str>> = panes.iter().map(|p| p.agent.as_deref()).collect();
        assert_eq!(
            got,
            [
                Some("claude"),
                Some("codex"),
                Some("claude"),
                Some("claude"),
                None
            ]
        );
    }
}
