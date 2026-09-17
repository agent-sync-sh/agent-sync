//! Tool configuration: `$XDG_CONFIG_HOME/agent-sync/agent-sync.toml` (default
//! `~/.config/agent-sync/`).
//!
//! Deliberately tiny, and deliberately *not* in the Commons — the Commons holds
//! ecosystem content that other tools may one day read, so nothing
//! agent-sync-specific squats there. An absent file means pure defaults.
//!
//! ```toml
//! [targets]
//! cursor = false            # a detected agent agent-sync should leave alone
//!
//! [custom.myagent]          # an agent the built-in registry does not know
//! root = ".myagent"
//! skills = ".myagent/skills"
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{DocumentMut, Item};

use crate::family::Family;
use serde_json::{Map, Value};

/// Where the config file lives inside the config directory.
pub const FILE: &str = "agent-sync.toml";
/// What the file was called before the rename — named by doctor, never read.
pub const LEGACY_FILE: &str = "agentstow.toml";

/// A target defined entirely by the user's config.
#[derive(Debug, Clone)]
pub struct CustomTarget {
    pub name: String,
    pub root: String,
    /// Home-relative fan-out directory per family.
    pub fanout: BTreeMap<Family, String>,
}

/// Per-server MCP settings: who gets it, and any agent-specific knobs.
///
/// Both live here rather than in the Commons because the Commons holds the
/// ecosystem-standard shape only.
#[derive(Debug, Clone, Default)]
pub struct McpRule {
    /// Agents allowed to receive this server. `None` means every capable agent.
    pub agents: Option<Vec<String>>,
    /// Native keys merged into one agent's rendering, by agent name.
    pub tweaks: BTreeMap<String, Map<String, Value>>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Registry agents the user switched off, by name.
    disabled: Vec<String>,
    custom: Vec<CustomTarget>,
    mcp: BTreeMap<String, McpRule>,
}

#[derive(Debug)]
pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Config {
    /// Read the config from a config directory. An absent file is not an error.
    pub fn load(config_dir: &Path) -> Result<Self, Error> {
        let path = config_dir.join(FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(Error {
                    message: format!("cannot read {}: {e}", path.display()),
                });
            }
        };

        // Never format the parse error: it quotes the offending source line, and
        // this file can hold adopted secrets in a Tweak.
        let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| Error {
            message: format!(
                "{} is not valid TOML ({})",
                path.display(),
                crate::mcp_toml::position_only(&text, &e)
            ),
        })?;

        let mut config = Self::default();

        if let Some(targets) = doc.get("targets").and_then(Item::as_table) {
            for (name, value) in targets.iter() {
                match value.as_bool() {
                    Some(false) => config.disabled.push(name.to_string()),
                    Some(true) => {}
                    None => {
                        return Err(Error {
                            message: format!(
                                "{}: targets.{name} must be true or false",
                                path.display()
                            ),
                        });
                    }
                }
            }
        }

        if let Some(custom) = doc.get("custom").and_then(Item::as_table) {
            for (name, value) in custom.iter() {
                let table = value.as_table().ok_or_else(|| Error {
                    message: format!("{}: custom.{name} must be a table", path.display()),
                })?;
                let root = table
                    .get("root")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error {
                        message: format!("{}: custom.{name} needs a `root`", path.display()),
                    })?
                    .to_string();

                let mut fanout = BTreeMap::new();
                for family in Family::ALL {
                    if let Some(dir) = table.get(family.name()).and_then(|v| v.as_str()) {
                        fanout.insert(*family, dir.to_string());
                    }
                }
                // The agents family had another name before v2; the old key
                // still works, with the new one winning when both appear.
                if !fanout.contains_key(&Family::Agents)
                    && let Some(dir) = table
                        .get(Family::PRE_V2_AGENTS_NAME)
                        .and_then(|v| v.as_str())
                {
                    fanout.insert(Family::Agents, dir.to_string());
                }

                config.custom.push(CustomTarget {
                    name: name.to_string(),
                    root,
                    fanout,
                });
            }
        }

        if let Some(item) = doc.get("mcp") {
            let mcp = item.as_table().ok_or_else(|| Error {
                message: format!(
                    "{}: `mcp` must be a table of servers written as [mcp.<name>]",
                    path.display()
                ),
            })?;
            for (server, value) in mcp.iter() {
                let table = value.as_table().ok_or_else(|| Error {
                    message: format!("{}: mcp.{server} must be a table", path.display()),
                })?;
                let mut rule = McpRule::default();

                for (key, item) in table.iter() {
                    match key {
                        "agents" => {
                            let array = item.as_array().ok_or_else(|| Error {
                                message: format!(
                                    "{}: mcp.{server}.agents must be a list of agent names",
                                    path.display()
                                ),
                            })?;
                            let mut names = Vec::new();
                            for entry in array.iter() {
                                let name = entry.as_str().ok_or_else(|| Error {
                                    message: format!(
                                        "{}: mcp.{server}.agents must hold agent names",
                                        path.display()
                                    ),
                                })?;
                                names.push(name.to_string());
                            }
                            rule.agents = Some(names);
                        }
                        "tweaks" => {
                            let tweaks = item.as_table().ok_or_else(|| Error {
                                message: format!(
                                    "{}: mcp.{server}.tweaks must be a table of agents",
                                    path.display()
                                ),
                            })?;
                            for (agent, knobs) in tweaks.iter() {
                                let table = knobs.as_table().ok_or_else(|| Error {
                                    message: format!(
                                        "{}: mcp.{server}.tweaks.{agent} must be a table",
                                        path.display()
                                    ),
                                })?;
                                let json = crate::mcp_toml::table_to_json(table);
                                rule.tweaks.insert(agent.to_string(), json);
                            }
                        }
                        // A closed key set: a typo here would otherwise be
                        // silently ignored for the life of the file.
                        other => {
                            return Err(Error {
                                message: format!(
                                    "{}: mcp.{server}.{other} is not a setting — \
                                     expected `agents` or `tweaks`",
                                    path.display()
                                ),
                            });
                        }
                    }
                }

                config.mcp.insert(server.to_string(), rule);
            }
        }

        Ok(config)
    }

    /// The rule for one server, if the user wrote one.
    pub fn mcp_rule(&self, server: &str) -> Option<&McpRule> {
        self.mcp.get(server)
    }

    /// Whether this server is meant for this agent.
    pub fn mcp_allows(&self, server: &str, agent: &str) -> bool {
        match self.mcp.get(server).and_then(|r| r.agents.as_ref()) {
            Some(allowed) => allowed.iter().any(|a| a == agent),
            None => true,
        }
    }

    /// Native knobs for one server on one agent.
    pub fn mcp_tweaks(&self, server: &str, agent: &str) -> Option<&Map<String, Value>> {
        self.mcp.get(server)?.tweaks.get(agent)
    }

    /// Every agent name mentioned anywhere in the MCP rules, for validation.
    pub fn mcp_agent_names(&self) -> Vec<(&str, &str)> {
        let mut names = Vec::new();
        for (server, rule) in &self.mcp {
            for agent in rule.agents.iter().flatten() {
                names.push((server.as_str(), agent.as_str()));
            }
            for agent in rule.tweaks.keys() {
                names.push((server.as_str(), agent.as_str()));
            }
        }
        names
    }

    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.iter().any(|d| d == name)
    }

    pub fn custom(&self) -> &[CustomTarget] {
        &self.custom
    }
}
