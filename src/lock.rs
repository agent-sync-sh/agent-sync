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

#[cfg(test)]
mod tests {
    use super::*;

    /// `try_exclusive` has one signature and two bodies — advisory `flock` on
    /// Unix, an empty share mode on Windows — so the same two tests prove both.
    /// On Unix a second `open` makes a new open file description, which is what
    /// `flock` contends on; on Windows the holder's share mode of 0 is what
    /// makes the second open fail. Different mechanisms, one contract.
    #[test]
    fn a_second_attempt_is_refused_while_the_first_holds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");

        let held = try_exclusive(&path)
            .unwrap()
            .expect("first caller acquires");

        let second = try_exclusive(&path).expect("a refusal is not an error");
        assert!(
            second.is_none(),
            "a second holder must be refused, not blocked"
        );

        drop(held);
    }

    #[test]
    fn the_lock_is_released_when_the_holder_drops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");

        let held = try_exclusive(&path)
            .unwrap()
            .expect("first caller acquires");
        drop(held);

        assert!(
            try_exclusive(&path).unwrap().is_some(),
            "dropping the handle must release it — this is what makes a crashed \
             run leave nothing to clean up"
        );
    }

    /// The lock file is created on demand, so a first-ever run works.
    #[test]
    fn the_lock_file_is_created_if_it_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        assert!(!path.exists());

        let held = try_exclusive(&path).unwrap().expect("acquires");
        assert!(path.exists(), "the lock file should now exist");

        drop(held);
    }
}
