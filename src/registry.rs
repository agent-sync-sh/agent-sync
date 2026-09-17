//! The Target registry: one row per known agent, declaring where it keeps each
//! config family and by which mechanism agent-sync reaches it.
//!
//! Adding an agent is a data change — one [`Agent`] row — never new control
//! flow. Detection is "the agent's config root exists": agent-sync creates
//! subdirectories under an existing root, but never a root itself, so an agent
//! that is not installed stays inert.

use std::path::{Path, PathBuf};

use crate::family::Family;

/// How an agent gets skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skills {
    /// Symlink fan-out into this home-relative directory.
    FanOut(&'static str),
    /// The agent reads the Commons directly; nothing to fan out. `legacy`
    /// names a fan-out directory an earlier registry used and the agent still
    /// reads, making our old links there live duplicates — sync visits it in
    /// prune-only mode: it removes links of ours, creates nothing, and touches
    /// nothing foreign.
    Native { legacy: Option<&'static str> },
    /// The agent has no skill surface.
    None,
}

/// How an agent gets the shared `AGENTS.md`.
///
/// A Foreign file occupying the destination is a *runtime* state (reported as a
/// conflict), not a separate mechanism — every mechanism below yields to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instructions {
    /// Symlink the Commons `AGENTS.md` to this home-relative path.
    Symlink(&'static str),
    /// Ensure an import line inside this home-relative user-owned file.
    ImportLine(&'static str),
    /// Drop an `AGENTS.md` symlink into this home-relative rules directory.
    RulesDirLink(&'static str),
    /// No user-scope instructions surface.
    None,
}

/// Wire format of the file an entry family is merged into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Toml,
}

/// Per-agent shape of one MCP server entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpDialect {
    /// `{type, command, args, env, url, headers}` — the de-facto standard.
    Standard,
    /// Codex: `headers` becomes `http_headers`; http and sse collapse to a URL server.
    Codex,
    /// OpenCode: `type` is local/remote, `command` is one array, `env` is `environment`.
    Opencode,
    /// Gemini: remote transport splits into `url` (SSE) vs `httpUrl` (HTTP).
    Gemini,
    /// Windsurf: the remote URL key is `serverUrl`.
    Windsurf,
}

/// How an agent gets MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mcp {
    /// Key-merge Managed servers into a native config file.
    KeyMerge {
        /// Home-relative config file.
        file: &'static str,
        /// Top-level key holding the server map.
        root_key: &'static str,
        format: Format,
        dialect: McpDialect,
    },
    /// The agent discovers other agents' MCP config; writing would double-configure.
    NativeViaDiscovery,
    /// The agent has no MCP surface.
    None,
}

/// How an agent gets slash commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commands {
    /// Symlink fan-out of Commons markdown into this home-relative directory.
    FanOut(&'static str),
    /// Render whole files (carrying the Marker) into this home-relative directory.
    Render { dir: &'static str, format: Format },
    /// The agent discovers other agents' command directories.
    NativeViaDiscovery,
    /// No user-scope command surface.
    None,
}

/// How an agent gets agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agents {
    /// Symlink fan-out into this home-relative directory.
    FanOut(&'static str),
    /// No subagent surface agent-sync targets in v1.
    None,
}

/// How an agent gets command-hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hooks {
    /// Key-merge hook elements into a native config file.
    ///
    /// No `format`: every hook surface is JSON, and carrying a field the code
    /// does not honour is how a TOML config once got a JSON document written
    /// into it.
    KeyMerge {
        file: &'static str,
        root_key: &'static str,
    },
    /// No hook surface agent-sync targets in v1.
    None,
}

/// One Target: an agent and everything agent-sync knows about reaching it.
#[derive(Debug, Clone, Copy)]
pub struct Agent {
    /// Stable identifier, used in reports and in `agent-sync.toml`.
    pub name: &'static str,
    /// Home-relative config root. Its existence *is* detection.
    pub root: &'static str,
    pub skills: Skills,
    pub instructions: Instructions,
    pub mcp: Mcp,
    pub commands: Commands,
    pub agents: Agents,
    pub hooks: Hooks,
}

impl Agent {
    /// Absolute config root under the given home directory.
    pub fn root_path(&self, home: &Path) -> PathBuf {
        home.join(self.root)
    }

    /// Whether this agent is installed: its config root exists.
    pub fn detected(&self, home: &Path) -> bool {
        self.root_path(home).is_dir()
    }

    /// The home-relative fan-out directory for a family, when this agent takes
    /// symlinks for it. `None` covers native agents and absent surfaces alike —
    /// both mean "do not fan out here".
    pub fn fanout_dir(&self, family: Family) -> Option<&'static str> {
        match family {
            Family::Skills => match self.skills {
                Skills::FanOut(dir) => Some(dir),
                _ => None,
            },
            Family::Commands => match self.commands {
                Commands::FanOut(dir) => Some(dir),
                _ => None,
            },
            Family::Agents => match self.agents {
                Agents::FanOut(dir) => Some(dir),
                _ => None,
            },
        }
    }

    /// The fan-out directory a Native agent once took for a family and still
    /// reads beside the Commons — visited by sync in prune-only mode, never
    /// written into. Only skills carry one today.
    pub fn legacy_dir(&self, family: Family) -> Option<&'static str> {
        match family {
            Family::Skills => match self.skills {
                Skills::Native { legacy } => legacy,
                _ => None,
            },
            Family::Commands | Family::Agents => None,
        }
    }

    /// Human-readable capability summary, in family order.
    pub fn capabilities(&self) -> Vec<(&'static str, String)> {
        vec![
            ("skills", describe_skills(self.skills)),
            ("instructions", describe_instructions(self.instructions)),
            ("mcp", describe_mcp(self.mcp)),
            ("commands", describe_commands(self.commands)),
            ("agents", describe_agents(self.agents)),
            ("hooks", describe_hooks(self.hooks)),
        ]
    }
}

fn describe_skills(c: Skills) -> String {
    match c {
        Skills::FanOut(dir) => format!("fan-out → {dir}"),
        Skills::Native { legacy: None } => "native (reads the Commons)".into(),
        Skills::Native { legacy: Some(dir) } => {
            format!("native (reads the Commons; cleans {dir})")
        }
        Skills::None => "none".into(),
    }
}

fn describe_instructions(c: Instructions) -> String {
    match c {
        Instructions::Symlink(p) => format!("symlink → {p}"),
        Instructions::ImportLine(p) => format!("import-line → {p}"),
        Instructions::RulesDirLink(d) => format!("rules-dir link → {d}"),
        Instructions::None => "none".into(),
    }
}

fn describe_mcp(c: Mcp) -> String {
    match c {
        Mcp::KeyMerge { file, .. } => format!("key-merge → {file}"),
        Mcp::NativeViaDiscovery => "native (via discovery)".into(),
        Mcp::None => "none".into(),
    }
}

fn describe_commands(c: Commands) -> String {
    match c {
        Commands::FanOut(dir) => format!("fan-out → {dir}"),
        Commands::Render { dir, .. } => format!("render → {dir}"),
        Commands::NativeViaDiscovery => "native (via discovery)".into(),
        Commands::None => "none".into(),
    }
}

fn describe_agents(c: Agents) -> String {
    match c {
        Agents::FanOut(dir) => format!("fan-out → {dir}"),
        Agents::None => "none".into(),
    }
}

fn describe_hooks(c: Hooks) -> String {
    match c {
        Hooks::KeyMerge { file, .. } => format!("key-merge → {file}"),
        Hooks::None => "none".into(),
    }
}

/// The built-in registry — the v1 capability matrix as data.
pub const AGENTS: &[Agent] = &[
    Agent {
        name: "claude",
        root: ".claude",
        skills: Skills::FanOut(".claude/skills"),
        // Claude users keep their own content in CLAUDE.md, so agent-sync adds
        // only the import line — never a symlink over the whole file.
        instructions: Instructions::ImportLine(".claude/CLAUDE.md"),
        mcp: Mcp::KeyMerge {
            file: ".claude.json",
            root_key: "mcpServers",
            format: Format::Json,
            dialect: McpDialect::Standard,
        },
        commands: Commands::FanOut(".claude/commands"),
        agents: Agents::FanOut(".claude/agents"),
        hooks: Hooks::KeyMerge {
            file: ".claude/settings.json",
            root_key: "hooks",
        },
    },
    Agent {
        name: "codex",
        root: ".codex",
        // Codex reads user-level ~/.agents/skills unconditionally — the
        // official skills doc lists it with no toggle
        // (https://developers.openai.com/codex/skills, verified 2026-08-16),
        // and codex-rs host_roots.rs ships it. The same source still reads
        // $CODEX_HOME/skills as a "deprecated user skills location, kept for
        // backward compat", so links we fanned out there are live duplicates,
        // not litter — sync prunes ours from the legacy dir.
        skills: Skills::Native {
            legacy: Some(".codex/skills"),
        },
        instructions: Instructions::Symlink(".codex/AGENTS.md"),
        mcp: Mcp::KeyMerge {
            file: ".codex/config.toml",
            root_key: "mcp_servers",
            format: Format::Toml,
            dialect: McpDialect::Codex,
        },
        commands: Commands::FanOut(".codex/prompts"),
        agents: Agents::None,
        hooks: Hooks::KeyMerge {
            file: ".codex/hooks.json",
            root_key: "hooks",
        },
    },
    Agent {
        name: "opencode",
        root: ".config/opencode",
        // OpenCode scans ~/.agents/skills natively. The Commons path is an interop
        // contract other agents hardcode (ADR-0004), not agent-sync's private
        // choice, so fan-out here would only create duplicate-name warnings.
        skills: Skills::Native { legacy: None },
        instructions: Instructions::Symlink(".config/opencode/AGENTS.md"),
        mcp: Mcp::KeyMerge {
            file: ".config/opencode/opencode.json",
            root_key: "mcp",
            format: Format::Json,
            dialect: McpDialect::Opencode,
        },
        commands: Commands::FanOut(".config/opencode/commands"),
        agents: Agents::FanOut(".config/opencode/agents"),
        hooks: Hooks::None,
    },
    Agent {
        name: "pi",
        root: ".pi",
        skills: Skills::FanOut(".pi/agent/skills"),
        instructions: Instructions::Symlink(".pi/agent/AGENTS.md"),
        // pi rejects MCP by design ("build CLI tools with READMEs").
        mcp: Mcp::None,
        commands: Commands::None,
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "oh-my-pi",
        root: ".omp",
        // Reads ~/.agents/skills directly (ADR-0004); it keeps no skills
        // directory of its own, so there is nothing to fan out into.
        skills: Skills::Native { legacy: None },
        instructions: Instructions::Symlink(".omp/agent/AGENTS.md"),
        mcp: Mcp::NativeViaDiscovery,
        commands: Commands::NativeViaDiscovery,
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "openclaw",
        root: ".openclaw",
        skills: Skills::FanOut(".openclaw/skills"),
        // Skills only for now. OpenClaw also keeps agents/, hooks/ and
        // extensions/ directories, but their formats are unverified and a
        // guessed target here writes into a real config file.
        instructions: Instructions::None,
        mcp: Mcp::None,
        commands: Commands::None,
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "hermes",
        root: ".hermes",
        // Hermes Agent discovers symlinked skills since it began walking with
        // `followlinks=True`. It can also read the Commons directly through its
        // `skills.external_dirs` setting — but that is user configuration
        // agent-sync does not write, and a capability the registry claims must be
        // true unconditionally, so this row states the mechanism that always
        // works.
        skills: Skills::FanOut(".hermes/skills"),
        // SOUL.md is the likely instructions surface; unverified, so unclaimed.
        instructions: Instructions::None,
        mcp: Mcp::None,
        commands: Commands::None,
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "gemini",
        root: ".gemini",
        // Gemini CLI reads user skills from ~/.gemini/skills or the
        // ~/.agents/skills alias, and `skills.enabled` defaults to true
        // (https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md,
        // verified 2026-08-16). agent-sync never fanned skills out to gemini,
        // so there is no legacy dir to clean.
        skills: Skills::Native { legacy: None },
        instructions: Instructions::Symlink(".gemini/GEMINI.md"),
        mcp: Mcp::KeyMerge {
            file: ".gemini/settings.json",
            root_key: "mcpServers",
            format: Format::Json,
            dialect: McpDialect::Gemini,
        },
        commands: Commands::Render {
            dir: ".gemini/commands",
            format: Format::Toml,
        },
        agents: Agents::None,
        hooks: Hooks::KeyMerge {
            file: ".gemini/settings.json",
            root_key: "hooks",
        },
    },
    Agent {
        name: "cursor",
        root: ".cursor",
        // Cursor loads skills automatically from user-level ~/.agents/skills/
        // (https://cursor.com/docs/context/skills, verified 2026-08-16) — and
        // also from the legacy-compat ~/.cursor/skills/, ~/.claude/skills/ and
        // ~/.codex/skills/, the worst duplication of all: a fanned-out link
        // here is read alongside the Commons entry it points at, so sync
        // prunes ours from the legacy dir instead.
        skills: Skills::Native {
            legacy: Some(".cursor/skills"),
        },
        // Cursor's user-level rules live in app-local storage, out of scope.
        instructions: Instructions::None,
        mcp: Mcp::KeyMerge {
            file: ".cursor/mcp.json",
            root_key: "mcpServers",
            format: Format::Json,
            dialect: McpDialect::Standard,
        },
        commands: Commands::FanOut(".cursor/commands"),
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "windsurf",
        root: ".codeium/windsurf",
        skills: Skills::None,
        instructions: Instructions::Symlink(".codeium/windsurf/memories/global_rules.md"),
        mcp: Mcp::KeyMerge {
            file: ".codeium/windsurf/mcp_config.json",
            root_key: "mcpServers",
            format: Format::Json,
            dialect: McpDialect::Windsurf,
        },
        commands: Commands::FanOut(".codeium/windsurf/global_workflows"),
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "roo",
        root: ".roo",
        skills: Skills::None,
        instructions: Instructions::RulesDirLink(".roo/rules"),
        // Roo's user-scope MCP lives in VS Code globalStorage — out of scope.
        mcp: Mcp::None,
        commands: Commands::FanOut(".roo/commands"),
        agents: Agents::None,
        hooks: Hooks::None,
    },
    Agent {
        name: "cline",
        root: ".cline",
        // Cline stays None: its skills docs
        // (https://docs.cline.bot/customization/skills, verified 2026-08-16)
        // list no .agents path at any level, the launch put skills behind an
        // opt-in toggle, and ~/.agents support exists only in undocumented
        // code that silently regressed once before — below ADR-0004's bar
        // that a registry row must be true unconditionally.
        skills: Skills::None,
        instructions: Instructions::None,
        mcp: Mcp::KeyMerge {
            file: ".cline/mcp.json",
            root_key: "mcpServers",
            format: Format::Json,
            dialect: McpDialect::Standard,
        },
        commands: Commands::None,
        agents: Agents::None,
        hooks: Hooks::None,
    },
];

/// Look up a registry row by name.
pub fn by_name(name: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.name == name)
}

/// Every agent whose config root exists under `home`.
pub fn detected(home: &Path) -> Vec<&'static Agent> {
    AGENTS.iter().filter(|a| a.detected(home)).collect()
}
