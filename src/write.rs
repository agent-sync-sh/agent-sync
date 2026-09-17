//! Atomic file replacement for the Rendered families.
//!
//! Rendered files hold resolved secrets and live in directories another program
//! is actively writing, so a half-written file is not an acceptable failure
//! mode. Every write goes to a private temp file beside the target and arrives
//! by `rename`, which is atomic on a POSIX filesystem.
//!
//! A symlinked target is resolved first — including a *dangling* one, whose
//! target does not exist yet. A dotfiles manager may own the real file
//! elsewhere, and replacing its link with a regular file would quietly break
//! that wiring precisely when the user could least afford it.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Mode for a file that may contain resolved secrets. Unix-only: Windows has
/// no mode bits, and inherited NTFS ACLs are the private-by-default story
/// there, so [`Written::mode`] reports zero and nothing is ever "exposed".
#[cfg(unix)]
const PRIVATE: u32 = 0o600;

/// Removes the temp file unless the write got all the way to `rename`.
struct Scratch {
    path: PathBuf,
    committed: bool,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The outcome of a write, for the caller to report on.
pub struct Written {
    /// The file that actually received the bytes, after following any symlink.
    pub path: PathBuf,
    /// Permission bits of that file once the write completed.
    pub mode: u32,
}

/// Replace `path`'s contents atomically, following it if it is a symlink.
///
/// An existing file keeps its permissions — silently tightening a file another
/// program owns is its own kind of surprise — and a new one is created private.
/// The caller is told the final mode so it can warn when secrets have landed
/// somewhere readable.
pub fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<Written> {
    let target = resolve(path);
    let parent = target.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;

    #[cfg(unix)]
    let mode = fs::metadata(&target)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(PRIVATE);
    #[cfg(windows)]
    let mode = 0;

    let scratch_path = scratch_path(&target);
    let mut scratch = Scratch {
        path: scratch_path.clone(),
        committed: false,
    };

    {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(PRIVATE);
        let mut file = opts.open(&scratch_path)?;
        file.write_all(bytes)?;
        // Durable before it is visible: rename must never expose a short file.
        file.sync_all()?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }

    fs::rename(&scratch_path, &target)?;
    scratch.committed = true;

    // The rename itself needs flushing, or a crash can lose the directory entry.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(Written { path: target, mode })
}

/// Follow a symlink to the file that should actually be rewritten.
///
/// `canonicalize` cannot help with a link whose target does not exist yet, so a
/// dangling link is followed lexically instead of being flattened into a
/// regular file.
fn resolve(path: &Path) -> PathBuf {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return path.to_path_buf();
    };
    if !meta.file_type().is_symlink() {
        return path.to_path_buf();
    }
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match fs::read_link(path) {
        Ok(text) => crate::link::resolve_link(path, &text),
        Err(_) => path.to_path_buf(),
    }
}

/// Whether a mode lets anyone but the owner read the file.
#[cfg(unix)]
pub fn is_exposed(mode: u32) -> bool {
    mode & 0o077 != 0
}

/// Windows access is ACL-inherited, not mode-carried; there is no bit here
/// that would make the warning truthful.
#[cfg(windows)]
pub fn is_exposed(_mode: u32) -> bool {
    false
}

/// A temp name beside the target. The pid keeps unrelated processes apart; the
/// global lock covers concurrent agent-sync runs on this machine.
fn scratch_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agent-sync".into());
    target.with_file_name(format!(".{name}.agent-sync-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_writes_the_bytes_and_leaves_no_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/file.json");

        let written = atomic(&target, b"{}").unwrap();

        assert_eq!(fs::read(&written.path).unwrap(), b"{}");
        let strays: Vec<_> = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "file.json")
            .collect();
        assert!(strays.is_empty(), "scratch file survived: {strays:?}");
    }

    /// The reported mode is what the caller warns on. Unix carries the existing
    /// file's bits forward; Windows has no mode to carry, and `atomic` reports 0
    /// rather than inventing one — see `is_exposed` for why that is honest.
    #[test]
    fn atomic_reports_the_mode_its_platform_can_know() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.json");

        #[cfg(unix)]
        {
            fs::write(&target, b"old").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
            let written = atomic(&target, b"new").unwrap();
            assert_eq!(
                written.mode, 0o644,
                "an existing file's mode must survive the replace"
            );
        }

        #[cfg(windows)]
        {
            let written = atomic(&target, b"new").unwrap();
            assert_eq!(written.mode, 0, "Windows has no mode bits to report");
        }
    }

    /// Unix answers from the bits; Windows cannot answer truthfully at all,
    /// because access there is ACL-inherited rather than mode-carried, so it
    /// always says no rather than emitting a warning it cannot stand behind.
    #[test]
    fn is_exposed_matches_what_the_platform_can_prove() {
        #[cfg(unix)]
        {
            assert!(!is_exposed(0o600), "owner-only is not exposed");
            assert!(is_exposed(0o644), "group and other read is exposed");
            assert!(is_exposed(0o604), "other read alone is exposed");
            assert!(!is_exposed(0o700));
        }

        #[cfg(windows)]
        {
            for mode in [0o600, 0o644, 0o604, 0o777, 0] {
                assert!(!is_exposed(mode), "Windows must never claim exposure");
            }
        }
    }

    /// A dangling symlink is followed lexically and the real file is written,
    /// rather than the link being replaced by a regular file.
    #[test]
    fn a_dangling_symlink_target_is_followed_not_flattened() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        let link = dir.path().join("link.json");
        crate::link::create_symlink(Path::new("real.json"), &link).unwrap();
        assert!(!real.exists(), "the target starts out missing");

        let written = atomic(&link, b"{}").unwrap();

        assert_eq!(written.path, real, "the real file should have been written");
        assert_eq!(fs::read(&real).unwrap(), b"{}");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link must survive as a link"
        );
    }
}
