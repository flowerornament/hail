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
fn a_headline_only_message_goes_in_as_its_envelope() {
    // hail-2en: the hook used to treat every headline-only message as already
    // typed, so mail to a seat with no pane was marked injected and never shown.
    let w = World::new();
    let id = w.send("the build is green");
    let d = stdout(&w.run("worker", &["deliver"]));
    assert_eq!(
        d.trim(),
        format!("[hail kind:fyi from:boss reply:boss id:{id}] the build is green")
    );
    assert!(stdout(&w.run("boss", &["sent", &id])).starts_with("injected "));
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

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_target_that_is_not_one_directory_name_is_refused() {
    let w = World::new();
    let before = files_under(&w.root.join("state"));
    for target in [
        "..",
        "../..",
        ".",
        "worker/../worker",
        "worker@..",
        "worker/..",
        "a/b/c",
    ] {
        let out = w.run("boss", &[target, "fyi", "x", "--no-wake"]);
        assert_eq!(out.status.code(), Some(1), "{target}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("is not a seat, sub-seat or pane"),
            "{target}: {out:?}"
        );
    }
    // Nothing was written anywhere: no message, no id, no new directory.
    assert_eq!(files_under(&w.root.join("state")), before);
    assert!(!w.root.join("state/new").exists());
    assert!(!w.state().join("new").exists());
}

#[test]
fn hail_seat_cannot_name_a_directory_outside_the_store() {
    let w = World::new();
    fs::create_dir_all(w.root.join("nowhere")).unwrap();
    for name in ["..", "."] {
        let out = w
            .hail("nowhere")
            .env("HAIL_SEAT", name)
            .args(["whoami"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3), "HAIL_SEAT={name}: {out:?}");
    }
}

#[test]
fn bd_is_gone_but_its_flags_say_so() {
    let w = World::new();
    // --bead is accepted and ignored, with a notice, so older sessions' sends
    // still go through.
    let out = w.run(
        "boss",
        &[
            "worker",
            "ask",
            "check x",
            "--bead",
            "hail-abc1",
            "--no-wake",
        ],
    );
    assert_eq!(out.status.code(), Some(5), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--bead is ignored"));
    let id = stdout(&out).trim().strip_prefix("id=").unwrap().to_string();
    let msg = fs::read_to_string(w.state().join(format!("seats/worker/new/{id}.md"))).unwrap();
    assert!(!msg.contains("bead:"), "{msg}");
    // note says what to use instead.
    let out = w.run("worker", &["note", "hail-abc1", "progress"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("bd comments add"));
}

#[test]
fn an_fyi_is_quiet_once_the_recipients_hooks_have_run() {
    let w = World::new();
    // Hooks never ran in worker: the fyi is typed as before (exit 5 here,
    // with no tmux), and the send is pending.
    let id = w.send("before hooks");
    assert!(w.state().join(format!("seats/boss/pending/{id}")).exists());
    // A deliver run by hand is not a hook (hail-zb5).
    assert!(w.run("worker", &["deliver"]).status.success());
    assert!(!w.state().join("seats/worker/hooked").exists());
    assert!(
        w.run("worker", &["deliver", "--format", "claude"])
            .status
            .success()
    );

    // Now they have: an fyi is quiet, exits 0, and leaves no pending record.
    let out = w.run("boss", &["worker", "fyi", "build green"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let id = stdout(&out).trim().strip_prefix("id=").unwrap().to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("quiet: arrives with worker's next prompt"),
        "{err}"
    );
    assert!(!w.state().join(format!("seats/boss/pending/{id}")).exists());
    assert!(stdout(&w.run("worker", &["deliver"])).contains("build green"));

    // Every other kind still tries to type (exit 5: no tmux here).
    let out = w.run("boss", &["worker", "ask", "review this"]);
    assert_eq!(out.status.code(), Some(5), "{out:?}");
}

fn header(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).unwrap();
    text.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix(&format!("{key}: ")).map(str::to_string))
}

fn id_of(o: &Output) -> String {
    let s = stdout(o);
    s.lines()
        .find_map(|l| l.strip_prefix("id="))
        .unwrap_or_else(|| panic!("no id: {o:?}"))
        .to_string()
}

#[test]
fn a_sub_agent_is_reached_through_its_parent_and_answers_as_itself() {
    let w = World::new();
    // To worker/scout: worker's mailbox, marked for the sub-agent.
    let out = w.run(
        "boss",
        &["worker/scout", "ask", "check the parser", "--no-wake"],
    );
    assert_eq!(out.status.code(), Some(5), "{out:?}");
    let id = id_of(&out);
    let msg = w.state().join(format!("seats/worker/new/{id}.md"));
    assert_eq!(header(&msg, "for").as_deref(), Some("scout"));
    // The obligation sits on the parent's mailbox, which relays and closes it.
    assert!(w.state().join(format!("seats/worker/owed/{id}")).is_file());

    // The sub-agent answers with --as: it signs as worker/scout.
    let out = w.run(
        "worker",
        &[
            "boss",
            "fyi",
            "parser checked",
            "--as",
            "scout",
            "--no-wake",
        ],
    );
    let answer = w.state().join(format!("seats/boss/new/{}.md", id_of(&out)));
    assert_eq!(header(&answer, "from").as_deref(), Some("worker/scout"));
    assert_eq!(header(&answer, "reply").as_deref(), Some("worker/scout"));

    // Its reply: value is a target that comes back marked for it.
    let out = w.run("boss", &["worker/scout", "fyi", "thanks", "--no-wake"]);
    let back = w
        .state()
        .join(format!("seats/worker/new/{}.md", id_of(&out)));
    assert_eq!(header(&back, "for").as_deref(), Some("scout"));

    // The parent's brief shows who each message is for.
    let brief = stdout(&w.run("worker", &["brief"]));
    assert!(brief.contains("for:scout"), "{brief}");
}

#[test]
fn sub_agent_names_and_near_misses_are_explained() {
    let w = World::new();
    let out = w.run("boss", &["worker-scout", "fyi", "x", "--no-wake"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("if it is a sub-agent of worker, send to worker/scout (its parent relays)"),
        "{err}"
    );
    let out = w.run(
        "worker",
        &["boss", "fyi", "x", "--as", "bad/name", "--no-wake"],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    // seat/%N needs the pane, so tmux; without it the send is refused, not misrouted.
    let out = w.run("boss", &["worker/%7", "fyi", "x", "--no-wake"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}

#[test]
fn exit_5_says_whether_anything_will_deliver_it() {
    // hail-2xl: mail to a mailbox no hook reads was promised "on their next
    // prompt" and sat for hours.
    let w = World::new();
    let out = w.run("boss", &["worker", "ask", "check x", "--no-wake"]);
    assert_eq!(out.status.code(), Some(5));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("No hook has read worker lately"), "{err}");
    assert!(err.contains("hail inbox in"), "{err}");
    assert!(!err.contains("arrives on their next prompt"), "{err}");

    assert!(
        w.run("worker", &["deliver", "--format", "claude"])
            .status
            .success()
    );
    // deliver exits 0 even when it fails; on failure say why.
    let hook_log = fs::read_to_string(w.state().join("hook-errors.log")).unwrap_or_default();
    assert!(
        w.state().join("seats/worker/hooked").exists(),
        "deliver --format left no hooked mark; hook-errors.log: {hook_log}"
    );
    let out = w.run("boss", &["worker", "ask", "check y", "--no-wake"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("it arrives on their next prompt"), "{err}");
}

#[test]
fn doctor_and_seats_show_mail_no_hook_will_deliver() {
    let w = World::new();
    w.send("waiting for nobody");
    let doctor = stdout(&w.run("boss", &["doctor"]));
    assert!(
        doctor.contains("worker has 1 unread and no hook has read it lately"),
        "{doctor}"
    );
    let seats = stdout(&w.run("boss", &["seats", "worker"]));
    assert!(seats.lines().next().unwrap().contains("HOOK"), "{seats}");
    assert!(seats.lines().nth(1).unwrap().contains(" - "), "{seats}");

    // Once the worker's prompt hook runs, its mail is delivered and both
    // views say so.
    w.send("now someone reads");
    assert!(
        w.run("worker", &["deliver", "--format", "claude"])
            .status
            .success()
    );
    let doctor = stdout(&w.run("boss", &["doctor"]));
    assert!(!doctor.contains("worker has"), "{doctor}");
    let seats = stdout(&w.run("boss", &["seats", "worker"]));
    assert!(seats.lines().nth(1).unwrap().contains("s "), "{seats}");
}
