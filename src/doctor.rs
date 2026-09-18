//! `doctor` — is this machine ready to sync?
//!
//! Reports what is installed and what would be silently skipped. It is strictly
//! read-only: doctor never creates a directory, least of all an agent root,
//! because a root's existence is what detection means.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::commons::{self, Commons};
use crate::config::{self, Config};
use crate::env::Env;
use crate::family::Family;
use crate::link;
use crate::registry;
use crate::registry::{Instructions, Skills};
use crate::report::Reporter;
use crate::target;

pub fn run(env: &Env, config: &Config, r: &mut Reporter) -> i32 {
    let commons = Commons::new(env.commons());

    r.line(format!("Commons {}", commons.root().display()));
    r.line(format!("Home    {}", env.home().display()));
    r.line(format!("Config  {}", env.config_dir().display()));
    r.line(format!("State   {}", env.state_dir().display()));
    r.blank();

    // Two leftovers, same shape: v1's ~/.agentstow and agentstow's XDG
    // directory. Both held agentstow.toml, both are named rather than migrated,
    // and neither is ever read.
    report_leftover(env.legacy_config_dir(), env, r);
    report_leftover(env.legacy_tool_config_dir(), env, r);

    if commons.exists() {
        report_commons(&commons, r);
    } else {
        r.problem(commons::missing_message(commons.root()));
    }

    // The agents family had another name before v2; a non-empty leftover
    // directory is silently invisible to sync, so name it.
    let pre_v2_agents = commons.root().join(Family::PRE_V2_AGENTS_NAME);
    if std::fs::read_dir(&pre_v2_agents).is_ok_and(|mut d| d.next().is_some()) {
        r.warn(format!(
            "{} is no longer a family — the agents family reads agents/; rename the directory",
            pre_v2_agents.display()
        ));
    }

    report_commons_override(env, config, r);
    report_agents(env, config, r);
    report_inert_imports(env, config, r);

    r.verdict()
}

/// An `@` import line naming the Commons in a file whose agent does not expand
/// one is silent: the agent loads no instructions and reports no error, which
/// is indistinguishable from working. Only Claude is measured to honor the
/// line (ADR-0008), so the line is named wherever else it turns up.
fn report_inert_imports(env: &Env, config: &Config, r: &mut Reporter) {
    let reference = crate::commons::INSTRUCTIONS.to_string();
    for target in target::resolve(env, config) {
        let Some(agent) = target.agent else {
            continue;
        };
        let file = match agent.instructions {
            Instructions::Symlink(rel) => env.in_home(rel),
            Instructions::RulesDirLink(dir) => env.in_home(dir).join(&reference),
            Instructions::IncludeEntry { legacy_link, .. } => match legacy_link {
                Some(rel) => env.in_home(rel),
                None => continue,
            },
            // The one agent that expands the line; nothing to warn about.
            Instructions::ImportLine(_) | Instructions::None => continue,
        };
        // A symlink into the Commons *is* the Commons file: any `@` line in it
        // is the user's own business, not an inert import.
        if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&file) else {
            continue;
        };
        let inert = body.lines().any(|l| {
            let l = l.trim();
            l.strip_prefix('@')
                .is_some_and(|rest| rest.trim().ends_with(&reference) && rest.contains(".agents"))
        });
        if inert {
            r.warn(format!(
                "{} holds an `@` import of the Commons, but {} does not expand import lines — it is silently ignored; the {} route is what reaches this agent",
                file.display(),
                agent.name,
                registry::describe_instructions(agent.instructions)
            ));
        }
    }
}

/// Name one directory the tool no longer reads, and say what to do with it.
///
/// Both leftovers kept their config as `agentstow.toml`, so that is the file to
/// look for. v1 kept its lock in the same directory, so a leftover holding no
/// config at all is the ordinary case — telling that user to move a file they
/// do not have reads as a broken instruction and sends them looking for
/// something that was never there.
fn report_leftover(dir: &Path, env: &Env, r: &mut Reporter) {
    if !dir.is_dir() {
        return;
    }
    if dir.join(config::LEGACY_FILE).is_file() {
        r.warn(format!(
            "{} is no longer read — move {} to {} as {} and delete the directory",
            dir.display(),
            config::LEGACY_FILE,
            env.config_dir().display(),
            config::FILE
        ));
    } else {
        r.warn(format!(
            "{} is no longer read and holds no {} — the directory can be deleted",
            dir.display(),
            config::LEGACY_FILE
        ));
    }
}

fn report_commons(commons: &Commons, r: &mut Reporter) {
    let scans: Vec<(Family, commons::Scan)> = Family::ALL
        .iter()
        .map(|family| (*family, commons.scan(*family)))
        .collect();

    let hooks = commons.family_dir(commons::HOOKS);
    let hook_files: Vec<String> = std::fs::read_dir(&hooks)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();

    let instructions = if commons.root().join(commons::INSTRUCTIONS).exists() {
        "present"
    } else {
        "absent"
    };
    let mcp = if commons.root().join(commons::MCP).exists() {
        "present"
    } else {
        "absent"
    };

    let sourced = sourced_entries(commons);

    r.line("Commons contents:");
    for (family, scan) in &scans {
        let n = sourced.iter().filter(|s| s.family == *family).count();
        if n > 0 {
            r.line(format!(
                "  {:<13} {} ({n} sourced)",
                family.name(),
                scan.entries.len()
            ));
        } else {
            r.line(format!("  {:<13} {}", family.name(), scan.entries.len()));
        }
    }
    if hooks.is_dir() {
        let count = hook_files.iter().filter(|n| n.ends_with(".toml")).count();
        r.line(format!("  hooks         {count}"));
    } else {
        r.line("  hooks         absent");
    }
    r.line(format!("  AGENTS.md     {instructions}"));
    r.line(format!("  mcp.json      {mcp}"));
    r.blank();

    // The machine-bootstrap question "what must I clone?" (ADR-0006): every
    // Sourced entry with its source, missing sources marked.
    if !sourced.is_empty() {
        r.line("Sourced entries:");
        for s in &sourced {
            let marker = if s.missing { " (source missing)" } else { "" };
            r.line(format!(
                "  {}/{} ← {}{marker}",
                s.family.name(),
                s.name,
                s.source.display()
            ));
        }
        r.blank();
    }

    let (protocol, others) = neighbours(commons);
    if !protocol.is_empty() {
        r.line(".agents Protocol surfaces:");
        for name in &protocol {
            r.line(format!("  {name}"));
        }
        r.blank();
    }
    if !others.is_empty() {
        r.line("Other tools in the Commons:");
        for name in &others {
            r.line(format!("  {name}"));
        }
        r.blank();
    }

    for (_, scan) in &scans {
        for issue in &scan.issues {
            r.warn(issue.to_string());
        }
    }

    // The hooks family reads `<Event>.toml` only, so anything else in there
    // would be skipped in silence — the failure this tool exists to end.
    for name in hook_files.iter().filter(|n| !n.ends_with(".toml")) {
        r.warn(format!(
            "Commons hooks/{name}: not a `<Event>.toml` file, skipped"
        ));
    }
}

/// One Sourced entry: a Commons symlink out to its Source.
struct Sourced {
    family: Family,
    name: String,
    /// Where the link points, resolved lexically — live or not.
    source: PathBuf,
    /// The source does not resolve: the repo is not cloned here (yet).
    missing: bool,
}

/// Every Sourced entry, gathered by reading the family directories directly:
/// the scan skips a dangling link (`Issue::DanglingLink`), and a Sourced entry
/// whose repo is not cloned yet is exactly what this view exists to name.
fn sourced_entries(commons: &Commons) -> Vec<Sourced> {
    let mut acc = Vec::new();
    for family in Family::ALL {
        let dir = commons.family_dir(family.name());
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Dot-prefixed names are never synced; the scan already warns.
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let is_link = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_link {
                continue;
            }
            let Ok(text) = std::fs::read_link(&path) else {
                continue;
            };
            acc.push(Sourced {
                family: *family,
                name,
                source: link::resolve_link(&path, &text),
                missing: std::fs::metadata(&path).is_err(),
            });
        }
    }
    acc.sort_by(|a, b| a.family.cmp(&b.family).then_with(|| a.name.cmp(&b.name)));
    acc
}

/// `.agents` Protocol paths recognized by name but not managed — a named kind
/// of Co-tenant (DECISIONS.md 2026-08-16). One becomes a family only when a
/// real consumer exists to fan out to.
const PROTOCOL_SURFACES: &[&str] = &["tasks", "memories", "models.json", "system-prompt.md"];

/// Names at the Commons root that are not agent-sync's own families, split into
/// Protocol surfaces (attributed) and other co-tenants (anonymous).
///
/// The Commons is a shared commons, not agent-sync's private directory (ADR-0004):
/// opencode, oh-my-pi and hermes read `~/.agents/` themselves, and the `skills`
/// CLI keeps its lock file there. An unrecognised name is a neighbour, not a
/// fault — so these are named and never counted, never called an issue, and
/// never touched. agent-sync can read filenames but not authorship, so it must
/// not claim how many *tools* are present, only which entries are not its own.
fn neighbours(commons: &Commons) -> (Vec<String>, Vec<String>) {
    let ours: Vec<&str> = Family::ALL
        .iter()
        .map(|f| f.name())
        .chain([commons::HOOKS, commons::INSTRUCTIONS, commons::MCP])
        .collect();

    let Ok(read) = std::fs::read_dir(commons.root()) else {
        return (Vec::new(), Vec::new());
    };
    let (mut protocol, mut others): (Vec<String>, Vec<String>) = read
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !ours.contains(&name.as_str()))
        .partition(|name| PROTOCOL_SURFACES.contains(&name.as_str()));
    protocol.sort();
    others.sort();
    (protocol, others)
}

/// Warn when a relocated Commons is invisible to the agents that read the
/// canonical path themselves.
///
/// `AGENT_SYNC_HOME` moves agent-sync's Commons, but it cannot move the path a
/// Native agent hardcodes. Those agents keep reading `~/.agents/` and silently
/// diverge from every agent that gets fan-out — so name them.
fn report_commons_override(env: &Env, config: &Config, r: &mut Reporter) {
    if env
        .var("AGENT_SYNC_HOME")
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return;
    }

    let canonical = env.home().join(crate::env::COMMONS_DIR);
    if link::normalize(env.commons()) == link::normalize(&canonical) {
        return;
    }

    let native: Vec<&str> = target::resolve(env, config)
        .iter()
        .filter_map(|t| t.agent)
        .filter(|a| matches!(a.skills, Skills::Native { .. }))
        .map(|a| a.name)
        .collect();
    if native.is_empty() {
        return;
    }

    r.warn(format!(
        "AGENT_SYNC_HOME points the Commons at {}, but {} read {} directly \
         and will not see it",
        env.commons().display(),
        native.join(" and "),
        canonical.display()
    ));
}

fn report_agents(env: &Env, config: &Config, r: &mut Reporter) {
    let home = env.home();
    let detected = target::resolve(env, config);
    // Disabled agents are not "known but missing" — the user switched them off,
    // so they leave the reckoning entirely.
    let known = registry::AGENTS
        .iter()
        .filter(|a| !config.is_disabled(a.name))
        .count()
        + config.custom().len();

    r.line(format!("Detected agents ({} of {known}):", detected.len()));
    if detected.is_empty() {
        r.line("  none — no agent config root found");
    }

    for agent in &detected {
        let families: Vec<String> = agent
            .capabilities()
            .into_iter()
            .filter(|(_, how)| how != "none")
            .map(|(family, how)| format!("{family} {how}"))
            .collect();
        r.line(format!("  {:<10} {}", agent.name, agent.root));
        for family in families {
            r.line(format!("               {family}"));
        }

        let root = home.join(&agent.root);
        if !is_writable(&root) {
            r.problem(format!(
                "{} config root is not writable: {}",
                agent.name,
                root.display()
            ));
        }
    }

    let missing = known.saturating_sub(detected.len());
    if missing > 0 {
        r.blank();
        r.line(format!(
            "{missing} known agents are not installed and were skipped."
        ));
    }
}

/// Ask the operating system, not the permission bits: a directory owned by
/// another user at 0755 is not read-only, yet we still cannot write in it.
#[cfg(unix)]
fn is_writable(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c_path is a valid NUL-terminated string for the duration of the call.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// Windows has no `access(2)` that answers ACLs honestly, so prove it by
/// doing: create and remove a scratch file. The one deliberate exception to
/// doctor being read-only — the probe never survives the call.
#[cfg(windows)]
fn is_writable(path: &Path) -> bool {
    let probe = path.join(format!(".agent-sync-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_writable` asks the operating system rather than reading permission
    /// bits — `access(2)` on Unix, a create-and-remove probe on Windows. Both
    /// answer the same three questions, so these tests run on every leg.
    #[test]
    fn a_writable_directory_says_yes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_writable(dir.path()));
    }

    #[test]
    fn a_missing_directory_says_no() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !is_writable(&dir.path().join("does-not-exist")),
            "a path that is not there cannot be written to"
        );
    }

    /// The Windows branch is the one deliberate exception to doctor being
    /// read-only: it writes a probe file to find out. The exception is only
    /// acceptable because the probe never outlives the call.
    #[test]
    fn the_probe_never_survives_the_call() {
        let dir = tempfile::tempdir().unwrap();

        assert!(is_writable(dir.path()));

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            leftovers.is_empty(),
            "doctor is read-only; the probe should be gone: {leftovers:?}"
        );
    }
}
