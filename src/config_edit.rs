//! Writing the tool config, `agent-sync.toml` in the XDG config directory.
//!
//! The user hand-writes this file, so edits are format-preserving in the same
//! way the Codex merge is: `toml_edit` keeps comments, key order and spacing,
//! and agent-sync touches only the tables it was asked to.

use serde_json::{Map, Value};
use toml_edit::{Array, DocumentMut, Item};

use crate::config::FILE;
use std::path::Path;

pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Read, mutate and write back the tool config, creating it if absent.
///
/// Nothing is written when the edit leaves the text unchanged, so a no-op
/// command does not touch the file's timestamp.
fn edit(
    config_dir: &Path,
    change: impl FnOnce(&mut DocumentMut) -> Result<(), Error>,
) -> Result<(), Error> {
    let path = config_dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(Error {
                message: format!("cannot read {}: {e}", path.display()),
            });
        }
    };

    let had_bom = text.starts_with('\u{feff}');
    let had_crlf = text.contains("\r\n");
    let body = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let mut doc: DocumentMut = if body.trim().is_empty() {
        DocumentMut::new()
    } else {
        body.parse().map_err(|e: toml_edit::TomlError| Error {
            // Never format the error itself: it quotes the offending line.
            message: format!(
                "{} is not valid TOML ({}) — left untouched",
                path.display(),
                crate::mcp_toml::position_only(body, &e)
            ),
        })?
    };

    change(&mut doc)?;

    let mut updated = doc.to_string();
    if had_crlf {
        updated = updated.replace('\n', "\r\n");
    }
    if had_bom {
        updated = format!("\u{feff}{updated}");
    }
    if updated == text {
        return Ok(());
    }

    // The config dir is created lazily: nothing else guarantees it exists,
    // since the lock lives in the state dir.
    std::fs::create_dir_all(config_dir).map_err(|e| Error {
        message: format!("cannot create {}: {e}", config_dir.display()),
    })?;
    crate::write::atomic(&path, updated.as_bytes()).map_err(|e| Error {
        message: format!("cannot write {}: {e}", path.display()),
    })?;
    Ok(())
}

/// Record which agents a server is for, and any agent-specific knobs.
pub fn set_mcp_rule(
    config_dir: &Path,
    server: &str,
    agents: Option<&[String]>,
    tweaks: &Map<String, Value>,
    agent: &str,
) -> Result<(), Error> {
    // Render the Tweak table before touching the file: a value TOML cannot hold
    // must abort the whole adoption, not vanish while the caller reports success.
    let rendered = if tweaks.is_empty() {
        None
    } else {
        Some(
            crate::mcp_toml::object_to_table(tweaks).map_err(|detail| Error {
                message: format!("mcp.{server}.tweaks.{agent}: {detail}"),
            })?,
        )
    };

    edit(config_dir, |doc| {
        let root = doc.as_table_mut();
        if root.get("mcp").is_none() {
            let mut parent = toml_edit::Table::new();
            parent.set_implicit(true);
            root.insert("mcp", Item::Table(parent));
        }
        let mcp = root
            .get_mut("mcp")
            .and_then(Item::as_table_mut)
            .ok_or_else(|| Error {
                message: "`mcp` in agent-sync.toml is not a table — left untouched".into(),
            })?;

        if mcp.get(server).is_none() {
            let mut entry = toml_edit::Table::new();
            entry.set_implicit(true);
            mcp.insert(server, Item::Table(entry));
        }
        let rule = mcp
            .get_mut(server)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| Error {
                message: format!("mcp.{server} in agent-sync.toml is not a table — left untouched"),
            })?;

        if let Some(agents) = agents {
            let mut array = Array::new();
            for name in agents {
                array.push(name.as_str());
            }
            // Reuse the existing key so the user's formatting and any comment
            // above it survive.
            match rule.key("agents").cloned() {
                Some(key) => {
                    rule.insert_formatted(&key, Item::Value(array.into()));
                }
                None => {
                    rule.insert("agents", Item::Value(array.into()));
                }
            }
            rule.set_implicit(false);
        }

        if let Some(mut table) = rendered {
            if rule.get("tweaks").is_none() {
                let mut parent = toml_edit::Table::new();
                parent.set_implicit(true);
                rule.insert("tweaks", Item::Table(parent));
            }
            let all = rule
                .get_mut("tweaks")
                .and_then(Item::as_table_mut)
                .ok_or_else(|| Error {
                    message: format!(
                        "mcp.{server}.tweaks in agent-sync.toml is not a table — left untouched"
                    ),
                })?;
            all.set_implicit(true);
            table.set_implicit(false);
            if let Some(previous) = all.get(agent).and_then(Item::as_table) {
                *table.decor_mut() = previous.decor().clone();
            }
            match all.key(agent).cloned() {
                Some(key) => {
                    all.insert_formatted(&key, Item::Table(table));
                }
                None => {
                    all.insert(agent, Item::Table(table));
                }
            }
        }

        Ok(())
    })
}

/// Forget a server entirely: allowlist, Tweaks and all.
pub fn remove_mcp_rule(config_dir: &Path, server: &str) -> Result<(), Error> {
    edit(config_dir, |doc| {
        if let Some(mcp) = doc.get_mut("mcp").and_then(Item::as_table_mut) {
            mcp.remove(server);
        }
        Ok(())
    })
}
