//! Which panes run an agent, and which one. tmux reports only a pane's
//! foreground command, which is the running tool while an agent is busy, so
//! the process table settles the rest.

use std::collections::HashMap;
use std::process::Command;

use super::tmux::Pane;

/// The agent a pane runs. Claude Code and Codex are known by name; any other
/// command in `HAIL_AGENT_COMMANDS` (the scenario harness uses `bash` and
/// `cat`) counts as an agent too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Other(String),
}

impl Agent {
    pub fn from_name(name: &str) -> Self {
        match name {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Other(n) => n,
        }
    }

    /// Whether this pane can be told apart from other agents in its
    /// directory (a sub-seat). Not Codex: its commands carry the shared
    /// daemon's `TMUX_PANE`, so hail cannot tell which Codex pane a command
    /// came from.
    pub fn can_hold_sub_seat(&self) -> bool {
        !matches!(self, Self::Codex)
    }
}

/// Commands that count as agents. `HAIL_AGENT_COMMANDS` overrides the
/// default.
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

/// Detect each pane's agent ([`mark_agents`]). One `ps`, only when some pane
/// is not settled by its foreground command alone.
pub fn detect(panes: &mut [Pane]) {
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
    pub fn parse(ps: &str) -> Self {
        let mut procs = Self::default();
        for line in ps.lines() {
            let mut f = line.split_whitespace();
            let (Some(own), Some(parent)) = (f.next(), f.next()) else {
                continue;
            };
            let (Ok(own), Ok(parent)) = (own.parse::<u32>(), parent.parse::<u32>()) else {
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
            procs.children.entry(parent).or_default().push(name.clone());
            procs.name.insert(own, name);
        }
        procs
    }
}

/// A pane runs an agent when tmux names its foreground command as one, when
/// its root process is one, or when its root has an agent as a direct child
/// (a shell that started the agent; tmux then reports the agent's running
/// tool as the foreground command, so the foreground check alone misses a
/// busy agent).
pub fn mark_agents(panes: &mut [Pane], agents: &[String], procs: &Procs) {
    let is_agent = |n: &&String| agents.contains(n);
    for p in panes.iter_mut() {
        let name = Some(&p.command)
            .filter(is_agent)
            .or_else(|| procs.name.get(&p.pid).filter(is_agent))
            .or_else(|| procs.children.get(&p.pid)?.iter().find(is_agent));
        p.agent = name.map(|n| Agent::from_name(n));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
        let got: Vec<Option<&str>> = panes
            .iter()
            .map(|p| p.agent.as_ref().map(Agent::name))
            .collect();
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
