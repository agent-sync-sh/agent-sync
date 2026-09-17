//! A global lock so a cron sync and a hand-run sync cannot interleave writes.
//!
//! The lock lives in the tool's state directory, never in the Commons.
//! It is released when the process exits — advisory `flock` on Unix, an
//! exclusive share mode on Windows — so a crashed run leaves nothing to
//! clean up.

use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::env::Env;

/// How long a mutating command waits for a competing one to finish.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// How long this invocation should wait, honouring the env override that lets
/// tests prove a busy lock fails cleanly without a 30 second pause.
pub fn timeout(env: &Env) -> Duration {
    let ms = env
        .var("AGENT_SYNC_LOCK_TIMEOUT_MS")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Duration::from_millis(ms)
}

/// Held for as long as the guard lives; released on drop.
pub struct Lock {
    _file: File,
}

#[derive(Debug)]
pub enum Error {
    /// Another agent-sync process held the lock for longer than we waited.
    Busy,
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Busy => write!(
                f,
                "another agent-sync process is running — try again when it finishes"
            ),
            Error::Io(e) => write!(f, "cannot take the agent-sync lock: {e}"),
        }
    }
}

/// Take the exclusive lock, waiting up to `timeout`.
pub fn acquire(state_dir: &Path, timeout: Duration) -> Result<Lock, Error> {
    fs::create_dir_all(state_dir).map_err(Error::Io)?;
    let path = state_dir.join("lock");

    let start = Instant::now();
    loop {
        match try_exclusive(&path) {
            Ok(Some(file)) => return Ok(Lock { _file: file }),
            Ok(None) => {}
            Err(e) => return Err(Error::Io(e)),
        }
        if start.elapsed() >= timeout {
            return Err(Error::Busy);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One attempt: the file held exclusively, `None` when someone else has it.
#[cfg(unix)]
fn try_exclusive(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::unix::io::AsRawFd;

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    // SAFETY: a live fd from the File above; flock only inspects it.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(Some(file));
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EINTR => Ok(None),
        _ => Err(err),
    }
}

/// Windows has no flock, but an open with an empty share mode refuses to
/// coexist with any other open of the same file — the holder's handle makes
/// every competitor fail with ERROR_SHARING_VIOLATION until it closes.
#[cfg(windows)]
fn try_exclusive(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;

    const ERROR_SHARING_VIOLATION: i32 = 32;
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
    {
        Ok(file) => Ok(Some(file)),
        Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => Ok(None),
        Err(e) => Err(e),
    }
}
