//! The instructions family: one `AGENTS.md` in the Commons, reaching every agent
//! by whichever mechanism that agent supports.
//!
//! Four mechanisms, because agents differ in kind and not just in path:
//! a plain symlink where the whole file can be ours; an **import-line** where
//! the file is the user's and we may only add one additive, idempotent line;
//! a **rules-dir link** where the agent globs a directory; and an
//! **include-entry** where the agent's own config lists the instruction files
//! it loads, so one element naming the Commons file is all it takes. Each is
//! offered from measured parser behavior, not from ownership alone — the
//! evidence is ADR-0008. Where an agent offers a symlink and an import route,
//! the user picks one in `agent-sync.toml`, and `sync` removes what the other
//! left of ours (ADR-0009).
//!
//! A Foreign file already occupying the destination is a conflict: agent-sync
//! reports it with a remediation hint and writes nothing. Resolving it is a
//! decision about someone else's content, which is not agent-sync's to make.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

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
    /// Our symlink, or our include entry, from a route this Target does not
    /// take — an earlier registry's, or the user's earlier choice (ADR-0009).
    /// Removed by `sync`; a real file there is not even named.
    Legacy,
    /// The other route's artifact of ours on this very path — Claude's
    /// `CLAUDE.md` as our link, or as a file holding only the import line —
    /// swapped by `sync` for the chosen route's (ADR-0009).
    Replace,
}

impl State {
    /// Whether `sync` will change anything for this state.
    pub fn needs_change(&self) -> bool {
        matches!(
            self,
            State::Missing | State::ImportMissing | State::Legacy | State::Replace
        )
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
            State::Legacy => "legacy",
            State::Replace => "replace",
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
    /// The list to hold one entry naming the Commons, for the include-entry
    /// mechanism.
    include: Option<Include>,
    /// The tool that appears to own a conflicting file, when recognisable.
    occupant: Option<String>,
    /// Why a Conflict is one, when it is not another tool's file.
    reason: Option<String>,
}

/// Where an include-entry lives and what it must say.
#[derive(Debug, Clone)]
struct Include {
    /// Dot-separated path to the array inside the JSON document.
    key: &'static str,
    /// The element agent-sync writes when none resolves to the Commons.
    entry: String,
    /// Kept as element 0 when the array is created (Gemini's memory target).
    keep_first: Option<&'static str>,
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
    /// Whether this item is an include-entry rather than a link or a line.
    pub fn is_include(&self) -> bool {
        self.include.is_some()
    }

    /// A human-facing explanation, including the fix where there is one.
    pub fn note(&self) -> String {
        match self.state {
            State::Linked => String::new(),
            State::Missing => "will be linked to the Commons".into(),
            State::ImportPresent if self.include.is_some() => {
                format!("included via {}", self.path.display())
            }
            State::ImportPresent => "imports the Commons instructions".into(),
            State::ImportMissing if self.include.is_some() => {
                format!("the entry will be added to {}", self.path.display())
            }
            State::ImportMissing => "the import line will be added".into(),
            State::Conflict => match (&self.reason, &self.occupant) {
                (Some(reason), _) => reason.clone(),
                (None, Some(tool)) => format!(
                    "{} is owned by {tool} — move or merge it, then sync again",
                    self.path.display()
                ),
                (None, None) => format!(
                    "{} is owned by something else — move or merge it, then sync again",
                    self.path.display()
                ),
            },
            State::Foreign => "points outside the Commons — left alone".into(),
            State::Legacy if self.include.is_some() => format!(
                "the entry in {} is from a route {} does not take — will be removed",
                self.path.display(),
                self.target
            ),
            State::Legacy => format!(
                "{} is a symlink into the Commons from a route {} does not take — will be removed",
                self.path.display(),
                self.target
            ),
            State::Replace if self.link_text.is_some() => {
                "holds only the import line — will be replaced by a link to the Commons".into()
            }
            State::Replace => {
                "is a link to the Commons — will be replaced by a file holding the import line"
                    .into()
            }
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
        let Some(chosen) = target.instructions else {
            continue;
        };
        let routes = target.agent.map_or(&[][..], |a| a.instructions);
        let item = match chosen {
            Instructions::Symlink(rel) => {
                let mut item = link_item(&target.name, env.in_home(rel), commons_file);
                // The import route on this same path leaves either a file of
                // ours, swapped for the link, or the user's own prose, which
                // is never touched — so the note names both ways out.
                if item.state == State::Conflict && routes.contains(&Instructions::ImportLine(rel))
                {
                    if holds_only_import(&item.path, env) {
                        item.state = State::Replace;
                    } else if item.occupant.is_none() {
                        item.reason = Some(format!(
                            "{} is a real file: move its text into the Commons AGENTS.md and sync again, \
                             or set instructions.{} = \"import\" in agent-sync.toml to keep it",
                            item.path.display(),
                            target.name
                        ));
                    }
                }
                item
            }
            Instructions::RulesDirLink(dir) => {
                let path = env.in_home(dir).join("AGENTS.md");
                link_item(&target.name, path, commons_file)
            }
            Instructions::ImportLine(rel) => import_item(&target.name, env.in_home(rel), env),
            Instructions::IncludeEntry {
                file,
                key,
                keep_first,
            } => include_item(&target.name, env.in_home(file), key, keep_first, env),
        };

        // What the routes not taken left behind goes, so the agent reads the
        // Commons once — but only while the chosen route is not blocked, so
        // it is never left with neither. A route on the chosen path is the
        // chosen item's own business.
        let blocked = matches!(item.state, State::Conflict | State::Foreign);
        let leftovers: Vec<Item> = routes
            .iter()
            .filter(|route| !blocked && **route != chosen)
            .filter_map(|route| leftover_item(&target.name, *route, env, commons_file))
            .filter(|leftover| leftover.path != item.path)
            .collect();
        items.push(item);
        items.extend(leftovers);
    }

    items
}

/// What a route this Target does not take left in place, if it is ours.
fn leftover_item(
    target: &str,
    route: Instructions,
    env: &Env,
    commons_file: &Path,
) -> Option<Item> {
    match route {
        Instructions::Symlink(rel) => legacy_item(target, env.in_home(rel), commons_file),
        Instructions::RulesDirLink(dir) => {
            legacy_item(target, env.in_home(dir).join("AGENTS.md"), commons_file)
        }
        Instructions::IncludeEntry {
            file,
            key,
            keep_first,
        } => {
            let item = include_item(target, env.in_home(file), key, keep_first, env);
            (item.state == State::ImportPresent).then_some(Item {
                state: State::Legacy,
                ..item
            })
        }
        // Only Claude has one, on the very path its symlink route takes.
        Instructions::ImportLine(_) => None,
    }
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
        include: None,
        occupant,
        reason: None,
    }
}

/// The path a link route the Target does not take would use. Only our own
/// link is an item: a real file there is the agent's (or another tool's) and
/// is no longer a destination, so it is not a Conflict; a link elsewhere is
/// Foreign in a directory we no longer narrate.
fn legacy_item(target: &str, path: PathBuf, commons_file: &Path) -> Option<Item> {
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let actual = fs::read_link(&path).ok()?;
    if link::resolve_link(&path, &actual) != link::normalize(commons_file) {
        return None;
    }
    Some(Item {
        target: target.to_string(),
        path,
        state: State::Legacy,
        link_text: None,
        import_line: None,
        include: None,
        occupant: None,
        reason: None,
    })
}

/// One entry naming the Commons file in the list of instruction files the
/// agent's own config declares. Presence is tested by resolving, so an entry the
/// user already wrote in another spelling counts and is never rewritten.
fn include_item(
    target: &str,
    path: PathBuf,
    key: &'static str,
    keep_first: Option<&'static str>,
    env: &Env,
) -> Item {
    let entry = include_reference(&path, keep_first, env);
    let commons_file = link::normalize(&env.commons().join(crate::commons::INSTRUCTIONS));
    let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let item = |state, reason| Item {
        target: target.to_string(),
        path: path.clone(),
        state,
        link_text: None,
        import_line: None,
        include: Some(Include {
            key,
            entry: entry.clone(),
            keep_first,
        }),
        occupant: None,
        reason,
    };

    let document = match read_document(&path) {
        Ok(document) => document,
        Err(reason) => return item(State::Conflict, Some(reason)),
    };
    if let Some(part) = blocked_segment(&document, key) {
        return item(
            State::Conflict,
            Some(format!(
                "{}: `{part}` is not an object — left untouched",
                path.display()
            )),
        );
    }
    let list = match lookup(&document, key) {
        None => return item(State::ImportMissing, None),
        Some(Value::Array(list)) => list.clone(),
        // Gemini documents `context.fileName` as string-or-array; a string is
        // one entry, promoted to an array on write.
        Some(Value::String(one)) => vec![Value::String(one.clone())],
        Some(_) => {
            return item(
                State::Conflict,
                Some(format!(
                    "{}: `{key}` is not a list — left untouched",
                    path.display()
                )),
            );
        }
    };

    let ours = |v: &Value| {
        v.as_str()
            .map(|s| resolve_entry(s, &base, env) == commons_file)
            .unwrap_or(false)
    };
    match list.iter().position(ours) {
        // Element 0 is the agent's own write target where `keep_first` says
        // so. The Commons there is a decision agent-sync will not reverse.
        Some(0) if keep_first.is_some() => item(
            State::Conflict,
            Some(format!(
                "{}: the Commons is first in `{key}`, where the agent writes its own memories — move it after {}",
                path.display(),
                keep_first.unwrap_or_default()
            )),
        ),
        Some(_) => item(State::ImportPresent, None),
        None => item(State::ImportMissing, None),
    }
}

/// How the Commons instructions are named inside a config list: the same
/// `~`-relative reference the import line uses, unless the list is joined
/// under the config file's own directory, where it must be relative to that.
fn include_reference(file: &Path, keep_first: Option<&'static str>, env: &Env) -> String {
    if keep_first.is_none() {
        return import_reference(env);
    }
    let commons_file = env.commons().join(crate::commons::INSTRUCTIONS);
    let base = file.parent().unwrap_or(Path::new("."));
    let rel = link::relative_from(base, &commons_file);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What one list element points at: `~/` expands to home, an absolute path
/// stands, and anything else is joined under the config file's directory —
/// the resolution opencode and Gemini each apply to their own list.
fn resolve_entry(text: &str, base: &Path, env: &Env) -> PathBuf {
    let path = if let Some(rest) = text.strip_prefix("~/") {
        env.home().join(rest)
    } else if Path::new(text).is_absolute() {
        PathBuf::from(text)
    } else {
        base.join(text)
    };
    link::normalize(&path)
}

/// A JSON document read the way the key-merge families read theirs: an
/// absent or blank file is an empty object; anything unparsable is a reason
/// to leave the file alone.
/// Blank or absent reads as an empty object; invalid JSON names the spot.
fn read_document(path: &Path) -> Result<Value, String> {
    crate::hooks::read_document(path).map_err(|e| e.message)
}

fn lookup<'a>(document: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(document, |node, part| node.get(part))
}

/// The first intermediate segment of `key` that exists but is not an object,
/// so the key can never be reached by creating what is missing.
fn blocked_segment<'k>(document: &Value, key: &'k str) -> Option<&'k str> {
    let parts: Vec<&str> = key.split('.').collect();
    let mut node = document;
    for part in &parts[..parts.len() - 1] {
        node = node.get(part)?;
        if !node.is_object() {
            return Some(part);
        }
    }
    None
}

/// The object that holds the last segment of `key`, created on the way down.
/// `None` when an intermediate segment is not an object.
fn lookup_parent_mut<'a>(
    document: &'a mut Value,
    key: &str,
) -> Option<(&'a mut Map<String, Value>, String)> {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop()?.to_string();
    let mut node = document;
    for part in parts {
        let map = node.as_object_mut()?;
        node = map
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    Some((node.as_object_mut()?, last))
}

/// Add the entry to the list, creating the file, the key path and the
/// `keep_first` sibling as needed. The entry is appended, never inserted.
fn add_entry(path: &Path, include: &Include) -> std::io::Result<()> {
    let mut document = read_document(path).map_err(std::io::Error::other)?;
    let (parent, last) = lookup_parent_mut(&mut document, include.key)
        .ok_or_else(|| std::io::Error::other(format!("`{}` is not reachable", include.key)))?;
    let mut list = match parent.remove(&last) {
        None => Vec::new(),
        Some(Value::Array(list)) => list,
        Some(Value::String(one)) => vec![Value::String(one)],
        Some(other) => {
            return Err(std::io::Error::other(format!(
                "`{}` is not a list ({other})",
                include.key
            )));
        }
    };
    // An empty list is as good as an absent key: appending to it would put
    // the Commons at element 0, the agent's own write target.
    if list.is_empty() {
        list.extend(
            include
                .keep_first
                .map(|first| Value::String(first.to_string())),
        );
    }
    list.push(Value::String(include.entry.clone()));
    parent.insert(last, Value::Array(list));
    write_document(path, &document)
}

/// Delete exactly the element that resolves to the Commons — the undo of the
/// include-entry. Every other element survives. When nothing but the
/// `keep_first` sibling (or nothing at all) remains, the key goes too, and so
/// does each parent object that is left empty, so the file reads as if
/// agent-sync had never been there. `Ok(true)` when an element was removed.
pub fn remove_include_entry(
    env: &Env,
    path: &Path,
    key: &str,
    keep_first: Option<&str>,
) -> std::io::Result<bool> {
    let mut document = match read_document(path) {
        Ok(document) => document,
        Err(_) if !path.exists() => return Ok(false),
        Err(reason) => return Err(std::io::Error::other(reason)),
    };
    let commons_file = link::normalize(&env.commons().join(crate::commons::INSTRUCTIONS));
    let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let Some(Value::Array(list)) = lookup(&document, key).cloned() else {
        return Ok(false);
    };
    let kept: Vec<Value> = list
        .iter()
        .filter(|v| {
            !v.as_str()
                .map(|s| resolve_entry(s, &base, env) == commons_file)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    if kept.len() == list.len() {
        return Ok(false);
    }

    let only_default = kept.is_empty()
        || (kept.len() == 1 && keep_first.is_some_and(|k| kept[0].as_str() == Some(k)));
    let parts: Vec<&str> = key.split('.').collect();
    if only_default {
        remove_key(&mut document, &parts);
    } else if let Some((parent, last)) = lookup_parent_mut(&mut document, key) {
        parent.insert(last, Value::Array(kept));
    }
    write_document(path, &document)?;
    Ok(true)
}

/// Remove the leaf at `parts`, then every ancestor object it left empty.
fn remove_key(document: &mut Value, parts: &[&str]) {
    let Some((last, rest)) = parts.split_last() else {
        return;
    };
    let Some(parent) = rest
        .iter()
        .try_fold(&mut *document, |node, part| node.get_mut(*part))
    else {
        return;
    };
    let Some(map) = parent.as_object_mut() else {
        return;
    };
    map.remove(*last);
    if map.is_empty() && !rest.is_empty() {
        remove_key(document, rest);
    }
}

fn write_document(path: &Path, document: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(document).map_err(std::io::Error::other)?;
    crate::write::atomic(path, body.as_bytes()).map(|_| ())
}

/// Claude keeps its own content in `CLAUDE.md`, so the whole file can never be
/// ours. The one sanctioned edit is adding an import line if it is not there.
fn import_item(target: &str, path: PathBuf, env: &Env) -> Item {
    let reference = import_reference(env);
    let line = format!("@{reference}");
    let mut reason = None;

    let state = if link::is_ours(&path, env.commons()) {
        // Our link here is the symlink route's, swapped for a file holding
        // the line. Appending through it would write into the Commons itself.
        let commons_file = env.commons().join(crate::commons::INSTRUCTIONS);
        let actual = fs::read_link(&path).unwrap_or_default();
        if link::resolve_link(&path, &actual) == link::normalize(&commons_file) {
            State::Replace
        } else {
            reason = Some(format!(
                "{} is a symlink into the Commons, so an import line would land in the Commons \
                 — replace the link with your own file",
                path.display()
            ));
            State::Conflict
        }
    } else {
        match fs::read_to_string(&path) {
            // Only a line that *is* the import counts. Prose mentioning the
            // path, or a commented-out import, must not suppress the real one.
            Ok(body) if body.lines().any(|l| is_import_of(l, &reference)) => State::ImportPresent,
            Ok(_) => State::ImportMissing,
            // No file yet: one containing just the import is a fine starting point.
            Err(_) => State::ImportMissing,
        }
    };

    Item {
        target: target.to_string(),
        path,
        state,
        link_text: None,
        import_line: Some(line),
        include: None,
        occupant: None,
        reason,
    }
}

/// Whether one line of a prose file is the import of `reference` — the single
/// test sync, revert and the route swap all use.
fn is_import_of(line: &str, reference: &str) -> bool {
    line.trim()
        .strip_prefix('@')
        .is_some_and(|rest| rest.trim() == reference)
}

/// A real file whose every non-blank line is our import holds nothing of the
/// user's: agent-sync wrote it, so it may give way to the symlink route.
fn holds_only_import(path: &Path, env: &Env) -> bool {
    let reference = import_reference(env);
    fs::read_to_string(path).is_ok_and(|body| {
        let mut lines = body.lines().filter(|l| !l.trim().is_empty()).peekable();
        lines.peek().is_some() && lines.all(|l| is_import_of(l, &reference))
    })
}

/// How the Commons instructions should be referred to inside a user's file:
/// `~`-relative when the Commons is under the home directory, absolute otherwise.
fn import_reference(env: &Env) -> String {
    let file = env.commons().join(crate::commons::INSTRUCTIONS);
    match file.strip_prefix(env.home()) {
        // The `~/` prefix already commits this reference to forward slashes, and
        // the line is written into a file another tool parses. Rendering the
        // remainder natively would emit `~/.agents\AGENTS.md` on Windows --
        // coherent as neither form, and not what the `~` convention means.
        Ok(rel) => {
            let rel: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!("~/{}", rel.join("/"))
        }
        // An absolute path is not a `~` reference and stays native.
        Err(_) => file.display().to_string(),
    }
}

/// Delete exactly the import line from a user-owned file — the undo of the one
/// sanctioned edit. Every other byte survives, line endings included, and the
/// file itself stays even when the line was all it held. `Ok(true)` when a
/// line was removed; a file that never had one (or does not exist) is `Ok(false)`.
pub fn remove_import_line(env: &Env, path: &Path) -> std::io::Result<bool> {
    // Through our link this file is the Commons, which is not the user's file.
    if link::is_ours(path, env.commons()) {
        return Ok(false);
    }
    let reference = import_reference(env);

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
        if is_import_of(piece, &reference) {
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
pub fn apply(env: &Env, item: &Item) -> std::io::Result<()> {
    match item.state {
        State::Missing => {
            let text = item.link_text.as_ref().expect("a link mechanism");
            if let Some(parent) = item.path.parent() {
                fs::create_dir_all(parent)?;
            }
            crate::link::create_symlink(text, &item.path)
        }
        State::ImportMissing if item.include.is_some() => {
            let include = item.include.as_ref().expect("an include mechanism");
            add_entry(&item.path, include)
        }
        State::Legacy => match &item.include {
            Some(include) => {
                remove_include_entry(env, &item.path, include.key, include.keep_first).map(|_| ())
            }
            None => crate::link::remove_symlink(&item.path),
        },
        // shortcut: the file goes before the link exists, so where links fail
        // (Windows without Developer Mode) only our line is lost and the error
        // is reported; link to a temp name and rename if Windows users hit it.
        State::Replace if item.link_text.is_some() => {
            fs::remove_file(&item.path)?;
            crate::link::create_symlink(item.link_text.as_ref().expect("a link"), &item.path)
        }
        State::Replace => {
            crate::link::remove_symlink(&item.path)?;
            append_import(item)
        }
        State::ImportMissing => append_import(item),
        _ => Ok(()),
    }
}

fn append_import(item: &Item) -> std::io::Result<()> {
    let line = item.import_line.as_ref().expect("an import mechanism");
    if let Some(parent) = item.path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut body = fs::read_to_string(&item.path).unwrap_or_default();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line);
    body.push('\n');
    fs::write(&item.path, body)
}
