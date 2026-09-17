//! The instructions family: one `AGENTS.md` in the Commons, reaching every agent
//! by whichever mechanism that agent supports.
//!
//! Three mechanisms, because agents differ in kind and not just in path:
//! a plain symlink where the whole file can be ours; an **import-line** where
//! the file is the user's and we may only add one additive, idempotent line;
//! and a **rules-dir link** where the agent globs a directory.
//!
//! A Foreign file already occupying the destination is a conflict: agent-sync
//! reports it with a remediation hint and writes nothing. Resolving it is a
//! decision about someone else's content, which is not agent-sync's to make.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::env::Env;
use crate::link;
use crate::registry::Instructions;
use crate::target;

/// What agent-sync found at one agent's instructions destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Our symlink is in place and canonical.
    Linked,
    /// Nothing there yet — a link will be created.
    Missing,
    /// The user's file already imports the Commons instructions.
    ImportPresent,
    /// The user's file exists (or does not) and needs the import line added.
    ImportMissing,
    /// A file we did not write occupies the destination.
    Conflict,
    /// A symlink pointing somewhere other than the Commons.
    Foreign,
}

impl State {
    /// Whether `sync` will change anything for this state.
    pub fn needs_change(&self) -> bool {
        matches!(self, State::Missing | State::ImportMissing)
    }

    /// A conflict is a decision for the user about another tool's file, so it
    /// is reported but never counted as something `sync` can resolve.
    pub fn actionable(&self) -> bool {
        self.needs_change()
    }

    pub fn label(&self) -> &'static str {
        match self {
            State::Linked => "linked",
            State::Missing => "missing",
            State::ImportPresent => "imported",
            State::ImportMissing => "import",
            State::Conflict => "conflict",
            State::Foreign => "foreign",
        }
    }
}

/// One agent's instructions destination and what to do about it.
#[derive(Debug, Clone)]
pub struct Item {
    pub target: String,
    pub path: PathBuf,
    pub state: State,
    /// Canonical link text, for the symlink mechanisms.
    link_text: Option<PathBuf>,
    /// The line to ensure, for the import-line mechanism.
    import_line: Option<String>,
    /// The tool that appears to own a conflicting file, when recognisable.
    occupant: Option<String>,
}

/// Markers other tools leave in files they generate, so a conflict can name the
/// culprit instead of shrugging. Unrecognised content simply stays anonymous.
const KNOWN_OCCUPANTS: &[(&str, &str)] = &[
    ("<claude-mem-context>", "claude-mem"),
    ("claude-mem", "claude-mem"),
];

fn identify_occupant(path: &Path) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let head: String = body.chars().take(2048).collect();
    KNOWN_OCCUPANTS
        .iter()
        .find(|(marker, _)| head.contains(marker))
        .map(|(_, tool)| (*tool).to_string())
}

impl Item {
    /// A human-facing explanation, including the fix where there is one.
    pub fn note(&self) -> String {
        match self.state {
            State::Linked => String::new(),
            State::Missing => "will be linked to the Commons".into(),
            State::ImportPresent => "imports the Commons instructions".into(),
            State::ImportMissing => "the import line will be added".into(),
            State::Conflict => match &self.occupant {
                Some(tool) => format!(
                    "{} is owned by {tool} — move or merge it, then sync again",
                    self.path.display()
                ),
                None => format!(
                    "{} is owned by something else — move or merge it, then sync again",
                    self.path.display()
                ),
            },
            State::Foreign => "points outside the Commons — left alone".into(),
        }
    }
}

/// Survey every detected Target's instructions destination.
///
/// Returns nothing at all when the Commons has no `AGENTS.md`: an absent family
/// is not a problem to report, it is simply a family the user does not use.
pub fn survey(env: &Env, config: &Config, commons_file: &Path) -> Vec<Item> {
    if !commons_file.is_file() {
        return Vec::new();
    }

    let mut items = Vec::new();

    for target in target::resolve(env, config) {
        let Some(agent) = target.agent else {
            // Config-defined targets take fan-out families only.
            continue;
        };

        let item = match agent.instructions {
            Instructions::Symlink(rel) => link_item(&target.name, env.in_home(rel), commons_file),
            Instructions::RulesDirLink(dir) => {
                let path = env.in_home(dir).join("AGENTS.md");
                link_item(&target.name, path, commons_file)
            }
            Instructions::ImportLine(rel) => import_item(&target.name, env.in_home(rel), env),
            Instructions::None => continue,
        };
        items.push(item);
    }

    items
}

fn link_item(target: &str, path: PathBuf, commons_file: &Path) -> Item {
    let parent = path.parent().unwrap_or(Path::new("."));
    let text = link::relative_from(parent, commons_file);

    let state = match fs::symlink_metadata(&path) {
        Err(_) => State::Missing,
        Ok(meta) if meta.file_type().is_symlink() => {
            let actual = fs::read_link(&path).unwrap_or_default();
            let resolved = link::resolve_link(&path, &actual);
            if resolved == link::normalize(commons_file) {
                State::Linked
            } else {
                State::Foreign
            }
        }
        Ok(_) => State::Conflict,
    };

    let occupant = if state == State::Conflict {
        identify_occupant(&path)
    } else {
        None
    };

    Item {
        target: target.to_string(),
        path,
        state,
        link_text: Some(text),
        import_line: None,
        occupant,
    }
}

/// Claude keeps its own content in `CLAUDE.md`, so the whole file can never be
/// ours. The one sanctioned edit is adding an import line if it is not there.
fn import_item(target: &str, path: PathBuf, env: &Env) -> Item {
    let reference = import_reference(env);
    let line = format!("@{reference}");

    let state = match fs::read_to_string(&path) {
        Ok(body) => {
            // Only a line that *is* the import counts. Prose mentioning the
            // path, or a commented-out import, must not suppress the real one.
            let imported = body.lines().any(|l| {
                let l = l.trim();
                l == line || (l.starts_with('@') && l[1..].trim() == reference)
            });
            if imported {
                State::ImportPresent
            } else {
                State::ImportMissing
            }
        }
        // No file yet: one containing just the import is a fine starting point.
        Err(_) => State::ImportMissing,
    };

    Item {
        target: target.to_string(),
        path,
        state,
        link_text: None,
        import_line: Some(line),
        occupant: None,
    }
}

/// How the Commons instructions should be referred to inside a user's file:
/// `~`-relative when the Commons is under the home directory, absolute otherwise.
fn import_reference(env: &Env) -> String {
    let file = env.commons().join(crate::commons::INSTRUCTIONS);
    match file.strip_prefix(env.home()) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => file.display().to_string(),
    }
}

/// Delete exactly the import line from a user-owned file — the undo of the one
/// sanctioned edit. Every other byte survives, line endings included, and the
/// file itself stays even when the line was all it held. `Ok(true)` when a
/// line was removed; a file that never had one (or does not exist) is `Ok(false)`.
pub fn remove_import_line(env: &Env, path: &Path) -> std::io::Result<bool> {
    let reference = import_reference(env);
    let line = format!("@{reference}");

    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    // The same test `import_item` uses to see the line, so what sync considers
    // present is exactly what revert removes.
    let mut kept = String::with_capacity(body.len());
    let mut removed = false;
    for piece in body.split_inclusive('\n') {
        let l = piece.trim_end_matches(['\n', '\r']).trim();
        if l == line || (l.starts_with('@') && l[1..].trim() == reference) {
            removed = true;
            continue;
        }
        kept.push_str(piece);
    }

    if !removed {
        return Ok(false);
    }
    fs::write(path, kept)?;
    Ok(true)
}

/// Bring one item to its intended state. Conflicts and Foreign links are no-ops.
pub fn apply(item: &Item) -> std::io::Result<()> {
    match item.state {
        State::Missing => {
            let text = item.link_text.as_ref().expect("a link mechanism");
            if let Some(parent) = item.path.parent() {
                fs::create_dir_all(parent)?;
            }
            crate::link::create_symlink(text, &item.path)
        }
        State::ImportMissing => {
            let line = item.import_line.as_ref().expect("an import mechanism");
            if let Some(parent) = item.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let existing = fs::read_to_string(&item.path).unwrap_or_default();
            let mut body = existing.clone();
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(line);
            body.push('\n');
            fs::write(&item.path, body)
        }
        _ => Ok(()),
    }
}
