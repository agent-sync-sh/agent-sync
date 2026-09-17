//! Link identity: the fan-out engine shared by every symlink family.
//!
//! Ownership is established by where a link *points*, not by any bookkeeping
//! (ADR-0001): a symlink resolving into the Commons is agent-sync's to canonicalise
//! or prune, and anything else — a link elsewhere, a real file or directory — is
//! left exactly as found.
//!
//! Resolution here is deliberately lexical. A dangling link still has to be
//! classified, and following it is impossible; comparing link text against the
//! Commons path textually gives the same answer for live and broken links alike.
//!
//! [`survey`] produces the whole picture of one Target directory; `sync` applies
//! the items that need changing and `status` reports all of them, so both
//! commands agree by construction.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::commons::Entry;

/// Resolve `.` and `..` textually, without touching the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    let mut root: Option<OsString> = None;
    let mut parts: Vec<OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => {
                root = Some(component.as_os_str().to_owned())
            }
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(last) if last != ".." => {
                    parts.pop();
                }
                // Above an absolute root there is nothing to pop.
                _ if root.is_some() => {}
                _ => parts.push("..".into()),
            },
            Component::Normal(part) => parts.push(part.to_owned()),
        }
    }

    let mut out = PathBuf::new();
    if let Some(root) = root {
        out.push(root);
    }
    for part in parts {
        out.push(part);
    }
    out
}

/// Create the symlink at `at` whose contents read `text`.
///
/// The one place symlinks are made, so the platform split lives nowhere else.
#[cfg(unix)]
pub fn create_symlink(text: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(text, at)
}

/// Windows symlinks are typed, so the flavour comes from what `text` resolves
/// to right now — a dangling target defaults to a file link, matching what
/// every Commons family links to when the entry is a file. Creation without
/// Developer Mode or elevation fails with os error 1314; that code is
/// translated into the fix, because the raw message names a privilege no
/// normal user has heard of.
#[cfg(windows)]
pub fn create_symlink(text: &Path, at: &Path) -> io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let make = if resolve_link(at, text).is_dir() {
        symlink_dir
    } else {
        symlink_file
    };
    make(text, at).map_err(explain_privilege)
}

/// Rewrite os error 1314 into the fix. Split out from `create_symlink` so it
/// can be tested without an unprivileged shell to fail in: an elevated session
/// (and a GitHub Windows runner) never produces 1314 to observe.
#[cfg(windows)]
fn explain_privilege(e: io::Error) -> io::Error {
    if e.raw_os_error() == Some(1314) {
        io::Error::new(
            e.kind(),
            "creating symlinks requires Windows Developer Mode \
             (Settings → System → For developers) or an elevated shell",
        )
    } else {
        e
    }
}

/// Where a symlink at `link` with contents `text` points, lexically.
pub fn resolve_link(link: &Path, text: &Path) -> PathBuf {
    if text.is_absolute() {
        normalize(text)
    } else {
        let base = link.parent().unwrap_or(Path::new(""));
        normalize(&base.join(text))
    }
}

/// The relative path from directory `base` to `target`.
///
/// Relative links are the canonical form: they survive the whole tree being
/// renamed or restored somewhere else, which absolute links do not.
pub fn relative_from(base: &Path, target: &Path) -> PathBuf {
    let base = normalize(base);
    let target = normalize(target);

    if base.is_absolute() != target.is_absolute() {
        return target;
    }

    let base_parts: Vec<Component> = base.components().collect();
    let target_parts: Vec<Component> = target.components().collect();
    let shared = base_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut out = PathBuf::new();
    for _ in shared..base_parts.len() {
        out.push("..");
    }
    for part in &target_parts[shared..] {
        out.push(part.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// What agent-sync found at one destination path, before deciding anything.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    Absent,
    /// A symlink pointing into the Commons — ours.
    Ours {
        text: PathBuf,
        resolved: PathBuf,
        dangling: bool,
    },
    /// A symlink pointing anywhere else — Foreign.
    Foreign,
    /// A real file or directory — a Variant, preserved unconditionally.
    Variant,
}

fn classify(path: &Path, commons: &Path) -> Found {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Found::Absent;
    };
    if !meta.file_type().is_symlink() {
        return Found::Variant;
    }
    let Ok(text) = fs::read_link(path) else {
        return Found::Variant;
    };

    let resolved = resolve_link(path, &text);
    if resolved.starts_with(normalize(commons)) {
        Found::Ours {
            text,
            resolved,
            dangling: !path.exists(),
        }
    } else {
        Found::Foreign
    }
}

/// Whether the object at `path` is a symlink of ours — one resolving into the
/// Commons. Lexical like [`classify`], so a dangling link answers the same as
/// a live one. Everything else — Foreign links, real files, real directories —
/// answers `false`.
pub fn is_ours(path: &Path, commons: &Path) -> bool {
    matches!(classify(path, commons), Found::Ours { .. })
}

/// The state of one name in one Target directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A canonical link onto the right Commons entry — nothing to do.
    Linked,
    /// The Commons has this entry and the Target does not.
    Missing,
    /// Our link, but not in canonical form (absolute, or onto the wrong entry).
    Stale,
    /// Our link, pointing at a Commons entry that no longer exists.
    Dangling,
    /// Our link in a legacy fan-out directory the agent reads *beside* the
    /// Commons — a live duplicate of the entry it points at, pruned so each
    /// skill is seen once.
    Duplicate,
    /// A real object shadowing a Commons entry, byte-identical to it.
    VariantIdentical,
    /// A real object shadowing a Commons entry, deliberately different.
    VariantDiverged,
    /// Not ours: a link resolving outside the Commons.
    Foreign,
}

impl State {
    /// Whether `sync` changes the filesystem for this state.
    pub fn needs_change(&self) -> bool {
        matches!(
            self,
            State::Missing | State::Stale | State::Dangling | State::Duplicate
        )
    }

    /// Whether the user has something to act on. A diverged Variant is a
    /// settled decision, and a Foreign entry is somebody else's; neither is.
    pub fn actionable(&self) -> bool {
        self.needs_change() || matches!(self, State::VariantIdentical)
    }

    /// Stable word used in reports and `--json`.
    pub fn label(&self) -> &'static str {
        match self {
            State::Linked => "linked",
            State::Missing => "missing",
            State::Stale => "stale",
            State::Dangling => "dangling",
            State::Duplicate => "duplicate",
            State::VariantIdentical => "variant-identical",
            State::VariantDiverged => "variant",
            State::Foreign => "foreign",
        }
    }

    /// One-line explanation for the human report.
    pub fn note(&self) -> &'static str {
        match self {
            State::Linked => "",
            State::Missing => "not linked yet",
            State::Stale => "not in canonical form",
            State::Dangling => "Commons entry is gone",
            State::Duplicate => "the agent reads the Commons natively — leftover fan-out link",
            State::VariantIdentical => "identical to the Commons — could be re-linked",
            State::VariantDiverged => "left alone",
            State::Foreign => "not ours — left alone",
        }
    }
}

/// One name in one Target directory, and what agent-sync makes of it.
#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub state: State,
    /// Canonical link text, when there is a Commons entry to point at.
    pub canonical: Option<PathBuf>,
}

/// Survey one Target directory against the Commons entries destined for it.
///
/// Pruning is limited to *dangling* links into the Commons: a live link onto a
/// Commons entry that simply is not being synced right now stays, because removing
/// it would be guessing.
pub fn survey(target_dir: &Path, commons: &Path, entries: &[Entry]) -> Vec<Item> {
    let mut items = Vec::new();
    let wanted: BTreeSet<&str> = entries.iter().map(|e| e.file_name.as_str()).collect();

    for entry in entries {
        let path = target_dir.join(&entry.file_name);
        let canonical = relative_from(target_dir, &entry.path);

        let state = match classify(&path, commons) {
            Found::Absent => State::Missing,
            Found::Ours { text, resolved, .. } => {
                if resolved == normalize(&entry.path) && text == canonical {
                    State::Linked
                } else {
                    State::Stale
                }
            }
            Found::Foreign => State::Foreign,
            Found::Variant => {
                if same_contents(&path, &entry.path) {
                    State::VariantIdentical
                } else {
                    State::VariantDiverged
                }
            }
        };

        items.push(Item {
            name: entry.name.clone(),
            path,
            state,
            canonical: Some(canonical),
        });
    }

    // Anything else in the directory. Our own dead links are ours to remove and
    // links pointing elsewhere are worth naming, but a real file or directory
    // here is simply the agent's own content — not a Variant of anything, and
    // none of our business.
    if let Ok(read) = fs::read_dir(target_dir) {
        for found in read.flatten() {
            let name = found.file_name().to_string_lossy().into_owned();
            if wanted.contains(name.as_str()) {
                continue;
            }
            let path = found.path();
            let state = match classify(&path, commons) {
                Found::Ours { dangling: true, .. } => State::Dangling,
                Found::Foreign => State::Foreign,
                _ => continue,
            };
            items.push(Item {
                name,
                path,
                state,
                canonical: None,
            });
        }
    }

    items.sort_by(|a, b| a.path.cmp(&b.path));
    items
}

/// Bring one item to its canonical state. States that need no change, and
/// everything that is not ours, are no-ops.
pub fn apply(item: &Item) -> io::Result<()> {
    match item.state {
        State::Missing => {
            let text = item
                .canonical
                .as_ref()
                .expect("missing implies a Commons entry");
            if let Some(parent) = item.path.parent() {
                fs::create_dir_all(parent)?;
            }
            create_symlink(text, &item.path)
        }
        State::Stale => {
            let text = item
                .canonical
                .as_ref()
                .expect("stale implies a Commons entry");
            fs::remove_file(&item.path)?;
            create_symlink(text, &item.path)
        }
        State::Dangling | State::Duplicate => fs::remove_file(&item.path),
        _ => Ok(()),
    }
}

/// Survey a legacy fan-out directory — one a Native agent still reads beside
/// the Commons. Prune-only: every link of ours, live or dangling, is a
/// [`State::Duplicate`] to remove; Foreign links and real objects are not even
/// named, because a directory agent-sync no longer fans out into is not its
/// business to narrate — only to leave. An absent directory surveys empty,
/// and nothing here ever creates one.
pub fn survey_legacy(target_dir: &Path, commons: &Path) -> Vec<Item> {
    let mut items = Vec::new();
    if let Ok(read) = fs::read_dir(target_dir) {
        for found in read.flatten() {
            let path = found.path();
            if let Found::Ours { .. } = classify(&path, commons) {
                items.push(Item {
                    name: found.file_name().to_string_lossy().into_owned(),
                    path,
                    state: State::Duplicate,
                    canonical: None,
                });
            }
        }
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items
}

/// Whether two paths hold byte-identical content — used to tell an accidental
/// duplicate from a deliberate divergence.
///
/// Anything unreadable answers `false`. Two unreadable trees must never compare
/// equal: callers treat "identical" as permission to delete one side.
pub fn same_contents(a: &Path, b: &Path) -> bool {
    match (a.is_dir(), b.is_dir()) {
        (true, true) => match (dir_digest(a), dir_digest(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        (false, false) => match (fs::read(a), fs::read(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        },
        _ => false,
    }
}

/// Sorted (relative path, bytes) listing of a directory tree.
fn dir_digest(root: &Path) -> Option<Vec<(String, Vec<u8>)>> {
    let mut acc = Vec::new();
    collect(root, root, &mut acc)?;
    acc.sort_by(|a, b| a.0.cmp(&b.0));
    Some(acc)
}

fn collect(root: &Path, dir: &Path, acc: &mut Vec<(String, Vec<u8>)>) -> Option<()> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(root).ok()?.display().to_string();
        if path.is_dir() {
            collect(root, &path, acc)?;
        } else {
            acc.push((rel, fs::read(&path).ok()?));
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows symlinks carry a flavour; Unix ones do not. Every test here runs
    /// on all three CI legs, and asserts the flavour only where one exists.
    #[cfg(windows)]
    use std::os::windows::fs::FileTypeExt;

    fn link_type(at: &Path) -> fs::FileType {
        fs::symlink_metadata(at)
            .expect("the link should exist")
            .file_type()
    }

    /// Windows decomposes an absolute path into Prefix("C:") followed by
    /// RootDir, so anything that treats the two as one slot keeps only the
    /// second and silently drops the drive.
    #[cfg(windows)]
    #[test]
    fn normalize_keeps_the_drive_letter() {
        let out = normalize(Path::new(r"C:\\Users\\x\\file.json"));
        assert_eq!(
            out,
            PathBuf::from(r"C:\\Users\\x\\file.json"),
            "the drive prefix must survive normalisation"
        );
    }

    #[cfg(windows)]
    #[test]
    fn resolve_link_keeps_the_drive_letter() {
        let out = resolve_link(Path::new(r"C:\\dir\\link"), Path::new("real.json"));
        assert_eq!(out, PathBuf::from(r"C:\\dir\\real.json"));
    }

    #[test]
    fn create_symlink_links_to_a_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("target.txt"), b"hi").unwrap();
        let at = dir.path().join("link");

        create_symlink(Path::new("target.txt"), &at).unwrap();

        let ft = link_type(&at);
        assert!(ft.is_symlink());
        #[cfg(windows)]
        assert!(ft.is_symlink_file(), "a file target must make a file link");
        assert_eq!(fs::read(&at).unwrap(), b"hi");
    }

    #[test]
    fn create_symlink_links_to_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/inside.txt"), b"hi").unwrap();
        let at = dir.path().join("link");

        create_symlink(Path::new("target"), &at).unwrap();

        let ft = link_type(&at);
        assert!(ft.is_symlink());
        #[cfg(windows)]
        assert!(
            ft.is_symlink_dir(),
            "a directory target must make a dir link, or traversal through it fails"
        );
        assert_eq!(fs::read(at.join("inside.txt")).unwrap(), b"hi");
    }

    /// The flavour is decided from what the text resolves to *now*, and a
    /// dangling target resolves to nothing — so it must fall back to a file
    /// link rather than failing. Every Commons family links to a file when the
    /// entry is a file, which is what makes file the right default.
    #[test]
    fn create_symlink_dangling_target_defaults_to_file_flavour() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("link");

        create_symlink(Path::new("nothing-here"), &at).unwrap();

        let ft = link_type(&at);
        assert!(ft.is_symlink(), "a dangling link is still a link");
        #[cfg(windows)]
        assert!(ft.is_symlink_file());
        assert!(!at.exists(), "and it still does not resolve");
        assert_eq!(fs::read_link(&at).unwrap(), Path::new("nothing-here"));
    }

    /// 1314 is ERROR_PRIVILEGE_NOT_HELD. Its own message names SeCreateSymbolicLink,
    /// a privilege no ordinary user has heard of, so it is replaced by the fix.
    #[cfg(windows)]
    #[test]
    fn a_privilege_error_is_rewritten_into_the_fix() {
        let mapped = explain_privilege(io::Error::from_raw_os_error(1314));
        let text = mapped.to_string();
        assert!(text.contains("Developer Mode"), "got: {text}");
        assert!(text.contains("elevated shell"), "got: {text}");
        assert!(
            !text.contains("1314"),
            "the raw code should not survive: {text}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn any_other_error_passes_through_untouched() {
        // 5 is ERROR_ACCESS_DENIED — a real failure the user must see verbatim.
        let original = io::Error::from_raw_os_error(5);
        let expected = original.to_string();
        let mapped = explain_privilege(io::Error::from_raw_os_error(5));
        assert_eq!(mapped.to_string(), expected);
        assert_eq!(mapped.raw_os_error(), Some(5));
    }
}
