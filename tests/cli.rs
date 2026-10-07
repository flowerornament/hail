//! End-to-end checks of the store's guarantees through the binary, without
//! tmux: sends use --no-wake, so only the filesystem is involved.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        for seat in ["boss", "worker"] {
            fs::create_dir_all(root.join(seat)).unwrap();
            fs::write(root.join(seat).join(".hail-seat"), seat).unwrap();
        }
        fs::create_dir_all(root.join("home")).unwrap();
        let w = Self { _tmp: tmp, root };
        // Each seat's agent has run hail once (its session-start hook does).
        for seat in ["boss", "worker"] {
            assert!(w.run(seat, &["whoami"]).status.success());
        }
        w
    }

    fn hail(&self, seat: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hail"));
        c.current_dir(self.root.join(seat))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("HOME", self.root.join("home"))
            .env("HAIL_SOCKET", "/nonexistent/hail-test-socket")
            .env("TMUX", "")
            .env_remove("TMUX_PANE")
            .env_remove("HAIL_SEAT")
            .stdin(Stdio::null());
        c
    }

    fn run(&self, seat: &str, args: &[&str]) -> Output {
        self.hail(seat).args(args).output().unwrap()
    }

    fn send(&self, headline: &str) -> String {
        let out = self.run("boss", &["worker", "fyi", headline, "--no-wake"]);
        assert_eq!(
            out.status.code(),
            Some(5),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        stdout.trim().strip_prefix("id=").unwrap().to_string()
    }

    fn state(&self) -> PathBuf {
        self.root.join("state/hail")
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn send_deliver_receipt() {
    let w = World::new();
    let mut child = w
        .hail("boss")
        .args(["worker", "ask", "--no-wake"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"review this\n\nthe body\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(5));
    let id = stdout(&out).trim().strip_prefix("id=").unwrap().to_string();
    assert_eq!(stdout(&w.run("boss", &["sent", &id])).trim(), "delivered");

    let d = w.run("worker", &["deliver", "--format", "claude"]);
    let v: serde_json::Value = serde_json::from_slice(&d.stdout).unwrap();
    let ctx = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("ask: review this") && ctx.contains("\n\nthe body"));
    assert!(stdout(&w.run("boss", &["sent", &id])).starts_with("injected "));
    assert!(
        stdout(&w.run("worker", &["deliver"])).is_empty(),
        "delivered once"
    );
}

#[test]
fn a_closed_stdout_gives_the_mail_back() {
    let w = World::new();
    let id = w.send("before the crash");
    // A hook killed before its output lands: fd 1 closed, the write fails.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("'{}' deliver >&-", env!("CARGO_BIN_EXE_hail")))
        .current_dir(w.root.join("worker"))
        .env("XDG_STATE_HOME", w.root.join("state"))
        .env("HOME", w.root.join("home"))
        .status()
        .unwrap();
    assert!(status.success());
    // Headline-only mail is claimed with no text to lose; send one with a body.
    let _ = id;
    let mut child = w
        .hail("boss")
        .args(["worker", "fyi", "--no-wake"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"with a body\nbody text\n")
        .unwrap();
    child.wait().unwrap();
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("'{}' deliver >&-", env!("CARGO_BIN_EXE_hail")))
        .current_dir(w.root.join("worker"))
        .env("XDG_STATE_HOME", w.root.join("state"))
        .env("HOME", w.root.join("home"))
        .status()
        .unwrap();
    assert!(status.success());
    let again = stdout(&w.run("worker", &["deliver"]));
    assert!(
        again.contains("body text"),
        "redelivered under the same id: {again:?}"
    );
}

#[test]
fn racing_hooks_claim_each_message_once() {
    let w = World::new();
    for i in 0..30 {
        let mut child = w
            .hail("boss")
            .args(["worker", "fyi", "--no-wake"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("m{i}\nbody {i}\n").as_bytes())
            .unwrap();
        child.wait().unwrap();
    }
    let children: Vec<_> = (0..8)
        .map(|_| {
            w.hail("worker")
                .arg("deliver")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let all: String = children
        .into_iter()
        .map(|c| stdout(&c.wait_with_output().unwrap()))
        .collect();
    for i in 0..30 {
        assert_eq!(
            all.matches(&format!("\nbody {i}\n")).count()
                + usize::from(all.ends_with(&format!("\nbody {i}"))),
            1,
            "body {i}"
        );
    }
}

#[test]
fn parallel_sends_never_share_an_id() {
    let w = World::new();
    let children: Vec<_> = (0..40)
        .map(|i| {
            w.hail("boss")
                .args(["worker", "fyi", &format!("n{i}"), "--no-wake"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut ids: Vec<String> = children
        .into_iter()
        .map(|c| stdout(&c.wait_with_output().unwrap()).trim().to_string())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 40);
    assert_eq!(
        fs::read_dir(w.state().join("seats/worker/new"))
            .unwrap()
            .count(),
        40
    );
}

#[test]
fn obligations_and_holds() {
    let w = World::new();
    let ask = w.run("boss", &["worker", "ask", "please do", "--no-wake"]);
    let id = stdout(&ask).trim().strip_prefix("id=").unwrap().to_string();
    assert!(stdout(&w.run("worker", &["brief"])).contains("open obligations on me (1)"));
    let wrong = w.run("boss", &["worker", "done", "x", "--re", &id, "--no-wake"]);
    assert_eq!(
        wrong.status.code(),
        Some(1),
        "only the obligated seat closes it"
    );
    let done = w.run(
        "worker",
        &["boss", "done", "did it", "--re", &id, "--no-wake"],
    );
    assert_eq!(done.status.code(), Some(5));
    assert!(!stdout(&w.run("worker", &["brief"])).contains("obligations"));
    assert!(!Path::new(&w.state().join("seats/worker/owed").join(&id)).exists());
}

#[test]
fn a_seat_name_bound_elsewhere_is_refused() {
    let w = World::new();
    let other = w.root.join("elsewhere/boss");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join(".hail-seat"), "boss").unwrap();
    assert_eq!(w.run("boss", &["whoami"]).status.code(), Some(0));
    let mut c = w.hail("boss");
    c.current_dir(&other);
    let out = c.arg("whoami").output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains(".hail-seat"));
}

#[test]
fn a_failing_claim_never_fails_the_hook() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let mut child = w
        .hail("boss")
        .args(["worker", "fyi", "--no-wake"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"held\nthe body\n")
        .unwrap();
    child.wait().unwrap();
    // cur/ refuses the rename that claims the message.
    let cur = w.state().join("seats/worker/cur");
    fs::create_dir_all(&cur).unwrap();
    fs::set_permissions(&cur, fs::Permissions::from_mode(0o555)).unwrap();
    let out = w.hail("worker").arg("deliver").output().unwrap();
    fs::set_permissions(&cur, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0");
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let log = fs::read_to_string(w.state().join("hook-errors.log")).unwrap();
    assert!(log.contains("deliver:"), "{log}");
    assert!(
        stdout(&w.run("worker", &["deliver"])).contains("the body"),
        "still unread, delivered next time"
    );
}

/// What agents read and copy, pinned: the help map, the send page, a brief
/// and the hook JSON. Ids and times are redacted.
mod snapshots {
    use super::*;

    fn redacted(f: impl FnOnce()) {
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"\d{4}T\d{6}-[0-9a-f]{4}", "[id]");
        settings.add_filter(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", "[time]");
        settings.bind(f);
    }

    #[test]
    fn help_pages() {
        let w = World::new();
        insta::assert_snapshot!("help_map", stdout(&w.run("boss", &["help"])));
        insta::assert_snapshot!("help_send", stdout(&w.run("boss", &["help", "send"])));
    }

    #[test]
    fn brief_and_hook_json() {
        let w = World::new();
        w.run(
            "boss",
            &[
                "worker",
                "ask",
                "review the auth change",
                "--scope",
                "auth",
                "--no-wake",
            ],
        );
        let mut child = w
            .hail("boss")
            .args(["worker", "fyi", "--no-wake"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"status with a body\nline one\nline two\n")
            .unwrap();
        child.wait().unwrap();
        w.run("boss", &["worker", "hold", "hold the merge", "--no-wake"]);
        redacted(|| {
            insta::assert_snapshot!("brief_worker", stdout(&w.run("worker", &["brief"])));
            insta::assert_snapshot!(
                "deliver_claude",
                stdout(&w.run("worker", &["deliver", "--format", "claude"]))
            );
        });
    }
}
