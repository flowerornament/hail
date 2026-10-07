//! Errors carry their exit code and end with what to run (spec §4.4, §10.2).

use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    /// Bad arguments or an unknown name. Exit 1.
    Usage(String),
    /// The state does not allow the action, or a file operation failed. Exit 1.
    State(String),
    /// A control-kind headline over the cap. Exit 2.
    OverCap(String),
    /// No seat here, a shared seat addressed bare, or a seat bound elsewhere. Exit 3.
    Seat(String),
    /// The target shows a permission dialog. Exit 4.
    Dialog(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) | Error::State(_) => 1,
            Error::OverCap(_) => 2,
            Error::Seat(_) => 3,
            Error::Dialog(_) => 4,
        }
    }

    /// An I/O failure on a path, as a state error that names the path.
    pub fn io(path: &Path, err: std::io::Error) -> Error {
        Error::State(format!("{}: {err}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(m)
            | Error::State(m)
            | Error::OverCap(m)
            | Error::Seat(m)
            | Error::Dialog(m) => f.write_str(m),
        }
    }
}

/// Exit code for a send that wrote the message but did not type it (spec §4.4).
pub const EXIT_NOT_WOKEN: u8 = 5;
