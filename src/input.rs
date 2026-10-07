//! Reading stdin without hanging: an agent's tool runner can leave stdin as
//! an open pipe that never ends, and a producer can run forever.

use std::io::{IsTerminal, Read};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::policy::{STDIN_FIRST_BYTE, STDIN_MAX, STDIN_TOTAL};

/// A file or /dev/null is readable at once.
const READY_WAIT: Duration = Duration::from_millis(10);

/// What stdin holds, if anything.
/// - A terminal is never read.
/// - A pipe or FIFO gets [`STDIN_FIRST_BYTE`] to start (bash passes even a
///   small heredoc through a pipe; its data is there at once).
/// - Anything else (a file, /dev/null) is read at once.
///
/// Once data flows it is read to EOF, refused past [`STDIN_TOTAL`] or
/// [`STDIN_MAX`]. Agents never pay the wait: under Claude Code 2.1 stdin is
/// /dev/null, under Codex 0.160 a pty (checked 2026-10-07).
pub fn read() -> Result<Option<Vec<u8>>> {
    if std::io::stdin().is_terminal() {
        return Ok(None);
    }
    read_if_ready(if is_pipe() {
        STDIN_FIRST_BYTE
    } else {
        READY_WAIT
    })
}

fn is_pipe() -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata("/dev/fd/0")
        .is_ok_and(|m| m.file_type().is_fifo() || m.file_type().is_socket())
}

enum Chunk {
    Data(Vec<u8>),
    End,
}

fn read_if_ready(first_byte: Duration) -> Result<Option<Vec<u8>>> {
    let (tx, rx) = mpsc::channel::<Chunk>();
    std::thread::spawn(move || {
        let mut lock = std::io::stdin().lock();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match lock.read(&mut buf) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Chunk::End);
                    return;
                }
                Ok(n) => {
                    if tx.send(Chunk::Data(buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let Ok(Chunk::Data(mut data)) = rx.recv_timeout(first_byte) else {
        return Ok(None);
    };
    let deadline = Instant::now() + STDIN_TOTAL;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Chunk::Data(d)) => {
                data.extend(d);
                if data.len() > STDIN_MAX {
                    return Err(Error::Usage(format!(
                        "the body on stdin is over {} MB; put it in a file and cite the path",
                        STDIN_MAX >> 20
                    )));
                }
            }
            Ok(Chunk::End) => return Ok(Some(data)),
            Err(_) => {
                return Err(Error::Usage(format!(
                    "stdin did not end within {} s; pass a finite body (a heredoc or a file)",
                    STDIN_TOTAL.as_secs()
                )));
            }
        }
    }
}
