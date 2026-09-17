//! Path resolution. Everything the tool touches is resolved from here, so the
//! two env overrides redirect the whole tool at a throwaway tree during tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the Commons lives when `AGENT_SYNC_HOME` is unset.
pub const COMMONS_DIR: &str = ".agents";
/// The tool's directory name inside each XDG base directory.
const TOOL_DIR: &str = "agent-sync";
/// Where tool configuration lived before v2 — named by doctor, never read.
pub const LEGACY_CONFIG_DIR: &str = ".agentstow";
/// The XDG subdirectory agentstow used before the rename — named by doctor,
/// never read. A third leftover layer, handled exactly like the other two:
/// say where it is and let the user delete it.
pub const LEGACY_TOOL_DIR: &str = "agentstow";

/// Resolved locations for one invocation.
#[derive(Debug, Clone)]
pub struct Env {
    /// Home directory for Target resolution — `AGENT_SYNC_TARGET_ROOT` or `HOME`.
    home: PathBuf,
    /// The Commons — `AGENT_SYNC_HOME`, else `<home>/.agents`.
    commons: PathBuf,
    /// Tool configuration — `$XDG_CONFIG_HOME/agent-sync`, else `<home>/.config/agent-sync`.
    config: PathBuf,
    /// Machine state (the lock) — `$XDG_STATE_HOME/agent-sync`, else `<home>/.local/state/agent-sync`.
    state: PathBuf,
    /// Where v1 kept its config — only doctor looks, to name the leftover.
    legacy_config: PathBuf,
    /// Where agentstow kept its config — only doctor looks, to name the leftover.
    legacy_tool_config: PathBuf,
    vars: BTreeMap<String, String>,
}

/// Neither `AGENT_SYNC_TARGET_ROOT` nor `HOME` was set, so nothing can resolve.
#[derive(Debug)]
pub struct NoHome;

impl std::fmt::Display for NoHome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "cannot determine home directory: set HOME (USERPROFILE on Windows) or AGENT_SYNC_TARGET_ROOT",
        )
    }
}

impl Env {
    /// Resolve from environment variables. `AGENT_SYNC_TARGET_ROOT` redirects
    /// home-relative Target resolution; `AGENT_SYNC_HOME` relocates the Commons
    /// independently, so tests can point them at different trees.
    pub fn resolve(vars: &[(String, String)]) -> Result<Self, NoHome> {
        let vars: BTreeMap<String, String> = vars.iter().cloned().collect();

        let home = vars
            .get("AGENT_SYNC_TARGET_ROOT")
            .or_else(|| vars.get("HOME"))
            // cmd and PowerShell set USERPROFILE, not HOME; Git Bash sets both.
            .or_else(|| vars.get("USERPROFILE"))
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or(NoHome)?;

        let commons = match vars.get("AGENT_SYNC_HOME").filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(v),
            None => home.join(COMMONS_DIR),
        };

        let config = xdg_dir(&vars, "XDG_CONFIG_HOME", &home, ".config", TOOL_DIR);
        let state = xdg_dir(&vars, "XDG_STATE_HOME", &home, ".local/state", TOOL_DIR);
        let legacy_config = home.join(LEGACY_CONFIG_DIR);
        let legacy_tool_config = xdg_dir(&vars, "XDG_CONFIG_HOME", &home, ".config", LEGACY_TOOL_DIR);

        Ok(Self {
            home,
            commons,
            config,
            state,
            legacy_config,
            legacy_tool_config,
            vars,
        })
    }

    /// The home directory Targets resolve against.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The Commons root.
    pub fn commons(&self) -> &Path {
        &self.commons
    }

    /// The tool configuration directory (never inside the Commons).
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// The state directory — the lock lives here, never in the Commons.
    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    /// Where v1 kept its config. Never read; doctor names a leftover one.
    pub fn legacy_config_dir(&self) -> &Path {
        &self.legacy_config
    }

    /// Where agentstow kept its config. Never read; doctor names a leftover one.
    pub fn legacy_tool_config_dir(&self) -> &Path {
        &self.legacy_tool_config
    }

    /// Resolve a home-relative path such as `.claude/skills`.
    pub fn in_home(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    /// Look up an environment variable, for `${env:VAR}` resolution.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }
}

/// One XDG base directory: the variable when set to an absolute path — the
/// spec says a relative value is invalid and must be ignored — else the
/// home-derived default. `leaf` is appended either way, so the same resolution
/// can name the current directory or the one agentstow left behind.
fn xdg_dir(
    vars: &BTreeMap<String, String>,
    var: &str,
    home: &Path,
    default_rel: &str,
    leaf: &str,
) -> PathBuf {
    let base = match vars.get(var).filter(|v| !v.is_empty()).map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => home.join(default_rel),
    };
    base.join(leaf)
}
