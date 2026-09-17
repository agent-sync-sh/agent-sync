//! The Commons: the one place a synced config lives.
//!
//! Scanning is deliberately strict about what it *cannot* see. The hand-rolled
//! script this tool replaces globbed `*/`, so dot-prefixed and non-directory
//! entries were skipped in silence; here every skipped entry becomes a warning.

use std::fs;
use std::path::{Path, PathBuf};

use crate::family::{Family, Shape};

/// Commons-relative directory of the hook definitions.
pub const HOOKS: &str = "hooks";
/// Commons-relative path of the shared instructions file.
pub const INSTRUCTIONS: &str = "AGENTS.md";
/// Commons-relative path of the canonical MCP server file.
pub const MCP: &str = "mcp.json";

/// One usable entry in a Commons family directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Display name: the directory name, or the file stem for file families.
    pub name: String,
    /// The name this entry must have in a Target — for markdown families that
    /// keeps the `.md` suffix, which agents' scanners require.
    pub file_name: String,
    pub path: PathBuf,
}

/// Something in the Commons that agent-sync will not sync, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// A dot-prefixed name: invisible to most agents' scanners, so never synced.
    DotPrefixed { family: Family, name: String },
    /// Wrong shape for the family (a file where a directory is required).
    WrongShape {
        family: Family,
        name: String,
        want: &'static str,
    },
    /// A symlink in the Commons whose own target is missing.
    DanglingLink { family: Family, name: String },
    /// The family directory could not be read.
    Unreadable { path: PathBuf, error: String },
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Issue::DotPrefixed { family, name } => write!(
                f,
                "Commons {family}/{name}: dot-prefixed names are invisible to agents and are never synced"
            ),
            Issue::WrongShape { family, name, want } => {
                write!(f, "Commons {family}/{name}: not a {want}, skipped")
            }
            Issue::DanglingLink { family, name } => write!(
                f,
                "Commons {family}/{name}: symlink target is missing, skipped"
            ),
            Issue::Unreadable { path, error } => {
                write!(f, "cannot read {}: {error}", path.display())
            }
        }
    }
}

/// What one family directory holds, plus everything skipped.
#[derive(Debug, Default)]
pub struct Scan {
    pub entries: Vec<Entry>,
    pub issues: Vec<Issue>,
}

/// What to tell the user when there is no Commons at all. One message, so the
/// advice cannot drift between commands.
pub fn missing_message(root: &Path) -> String {
    format!(
        "no Commons at {} — run `agent-sync init` to create one",
        root.display()
    )
}

/// The canonical Commons.
#[derive(Debug, Clone)]
pub struct Commons {
    root: PathBuf,
}

impl Commons {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    /// Absolute path of a family directory.
    pub fn family_dir(&self, family: &str) -> PathBuf {
        self.root.join(family)
    }

    /// Scan one family directory according to its shape.
    pub fn scan(&self, family: Family) -> Scan {
        let shape = family.shape();
        let dir = self.family_dir(family.name());
        let mut scan = Scan::default();

        let read = match fs::read_dir(&dir) {
            Ok(r) => r,
            // An absent family directory is simply an empty family, not a fault.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return scan,
            Err(e) => {
                scan.issues.push(Issue::Unreadable {
                    path: dir,
                    error: e.to_string(),
                });
                return scan;
            }
        };

        for entry in read.flatten() {
            let path = entry.path();
            let raw = entry.file_name().to_string_lossy().into_owned();

            if raw.starts_with('.') {
                scan.issues.push(Issue::DotPrefixed { family, name: raw });
                continue;
            }

            // symlink_metadata does not follow; metadata does. A Commons entry may
            // legitimately be a symlink into a git repo, so judge the target.
            let link = fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if link && fs::metadata(&path).is_err() {
                scan.issues.push(Issue::DanglingLink { family, name: raw });
                continue;
            }

            match shape {
                Shape::Directory => {
                    if path.is_dir() {
                        scan.entries.push(Entry {
                            name: raw.clone(),
                            file_name: raw,
                            path,
                        });
                    } else {
                        scan.issues.push(Issue::WrongShape {
                            family,
                            name: raw,
                            want: "directory",
                        });
                    }
                }
                Shape::Markdown => {
                    if path.is_dir() {
                        scan.issues.push(Issue::WrongShape {
                            family,
                            name: raw,
                            want: "markdown file",
                        });
                    } else if raw.ends_with(".md") {
                        scan.entries.push(Entry {
                            name: raw.trim_end_matches(".md").to_string(),
                            file_name: raw,
                            path,
                        });
                    } else {
                        scan.issues.push(Issue::WrongShape {
                            family,
                            name: raw,
                            want: "markdown file",
                        });
                    }
                }
            }
        }

        scan.entries.sort_by(|a, b| a.name.cmp(&b.name));
        scan.issues.sort_by_key(|i| format!("{i}"));
        scan
    }
}
