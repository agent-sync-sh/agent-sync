//! `revert <agent>` — deliberate offboarding: remove everything agent-sync put
//! into one target, and nothing else.
//!
//! It refuses while the target is still enabled, printing the exact
//! `targets.<name> = false` line to add: the forced order makes the intent
//! durable in config before the teardown, so a later sync cannot silently redo
//! what revert undid, and revert never edits the user's config as a side
//! effect. It is the one command that reaches a disabled agent — the gap it
//! exists to close, since disabling removes an agent from every scan and would
//! otherwise orphan its links forever.
//!
//! Ownership is the same three identities every other command uses: a symlink
//! resolving into the Commons, an MCP entry whose name is in the Commons, a
//! hook element whose command matches a Commons declaration, a rendered file
//! carrying the Marker. Foreign content and Variants are untouched, and the
//! agent's directories stay — they are detection roots, or the agent's own.

use std::fs;
use std::path::Path;

use crate::commons::{self, Commons};
use crate::config::{self, Config};
use crate::env::Env;
use crate::family::Family;
use crate::hooks;
use crate::instructions;
use crate::link;
use crate::lock;
use crate::mcp;
use crate::registry::{Commands, Hooks, Instructions};
use crate::render;
use crate::report::Reporter;
use crate::target::{self, Target};
use crate::{EXIT_CLEAN, EXIT_ERROR};

pub fn run(env: &Env, config: &Config, r: &mut Reporter, name: &str) -> i32 {
    // Refusals first, all before the lock: a refused revert holds nothing up.
    let Some(target) = target::find(config, name) else {
        r.problem(format!("`{name}` is not an agent agent-sync knows about"));
        return EXIT_ERROR;
    };

    if !config.is_disabled(name) {
        r.problem(format!(
            "{name} is still enabled — set targets.{name} = false in {} first, \
             so sync will not redo what revert undoes",
            env.config_dir().join(config::FILE).display()
        ));
        return EXIT_ERROR;
    }

    if !target.detected(env.home()) {
        r.line(format!(
            "{name} is not installed on this machine — nothing to revert"
        ));
        return EXIT_CLEAN;
    }

    let _lock = match lock::acquire(env.state_dir(), lock::timeout(env)) {
        Ok(guard) => guard,
        Err(e) => {
            r.problem(e.to_string());
            return EXIT_ERROR;
        }
    };

    let commons = Commons::new(env.commons());
    let mut removed = 0usize;

    // Family by family, in the order sync writes them. A flipped agent's
    // legacy fan-out dir still holds links of ours to strip — "everything
    // agent-sync put into one target" includes what an earlier registry put.
    for family in Family::ALL {
        let Some(dir) = target
            .fanout_dir(*family)
            .or_else(|| target.legacy_dir(*family))
        else {
            continue;
        };
        removed += remove_links(env, &env.in_home(dir), commons.root(), r);
    }
    removed += revert_instructions(env, &target, commons.root(), r);
    removed += revert_mcp(env, &target, &commons, r);
    removed += revert_rendered(env, &target, r);
    removed += revert_hooks(env, &target, &commons, r);

    if removed == 0 && r.problem_count() == 0 {
        r.line(format!(
            "nothing of agent-sync's in {name} — nothing to revert"
        ));
    } else if removed > 0 {
        r.blank();
        let noun = if removed == 1 { "removal" } else { "removals" };
        r.line(format!("{removed} {noun} made."));
    }

    if r.problem_count() > 0 {
        EXIT_ERROR
    } else {
        EXIT_CLEAN
    }
}

/// The instructions surface: undo whichever mechanism this agent takes.
fn revert_instructions(env: &Env, target: &Target, commons: &Path, r: &mut Reporter) -> usize {
    let Some(agent) = target.agent else {
        // Config-defined targets take fan-out families only.
        return 0;
    };
    match agent.instructions {
        Instructions::Symlink(rel) => remove_our_link(env, &env.in_home(rel), commons, r),
        Instructions::RulesDirLink(dir) => {
            let path = env.in_home(dir).join(commons::INSTRUCTIONS);
            remove_our_link(env, &path, commons, r)
        }
        Instructions::ImportLine(rel) => {
            let path = env.in_home(rel);
            match instructions::remove_import_line(env, &path) {
                Ok(true) => {
                    r.line(format!("removed the import line from {rel}"));
                    1
                }
                Ok(false) => 0,
                Err(e) => {
                    r.problem(format!("cannot rewrite {}: {e}", path.display()));
                    0
                }
            }
        }
        Instructions::IncludeEntry {
            file,
            key,
            keep_first,
            legacy_link,
        } => {
            let mut removed = 0;
            if let Some(rel) = legacy_link {
                removed += remove_our_link(env, &env.in_home(rel), commons, r);
            }
            let path = env.in_home(file);
            match instructions::remove_include_entry(env, &path, key, keep_first) {
                Ok(true) => {
                    r.line(format!("removed the {key} entry from {file}"));
                    removed += 1;
                }
                Ok(false) => {}
                Err(e) => r.problem(format!("cannot rewrite {}: {e}", path.display())),
            }
            removed
        }
        Instructions::None => 0,
    }
}

/// Managed MCP entries: every name the Commons defines, stripped from this one
/// agent's config the way `mcp remove` strips it from all of them.
fn revert_mcp(env: &Env, target: &Target, commons: &Commons, r: &mut Reporter) -> usize {
    let Some((path, root_key, format)) = crate::mcp_cmd::mcp_destination(env, target) else {
        return 0;
    };
    let servers = match mcp::commons_servers(&commons.root().join(commons::MCP)) {
        Ok(servers) => servers,
        Err(e) => {
            r.problem(e.to_string());
            return 0;
        }
    };

    let mut removed = 0usize;
    for server in servers.keys() {
        match mcp::strip(&path, root_key, format, server) {
            Ok(Some(_)) => {
                r.line(format!(
                    "removed mcp server {server} from {}",
                    rel_display(env, &path)
                ));
                removed += 1;
            }
            Ok(None) => {}
            Err(e) => r.problem(e.to_string()),
        }
    }
    removed
}

/// Rendered whole files: the Marker is proof we wrote them.
fn revert_rendered(env: &Env, target: &Target, r: &mut Reporter) -> usize {
    let Some(agent) = target.agent else {
        return 0;
    };
    let Commands::Render { dir, .. } = agent.commands else {
        return 0;
    };

    let mut removed = 0usize;
    for path in entries_of(&env.in_home(dir)) {
        let ours = fs::read_to_string(&path)
            .map(|text| render::is_ours(&text))
            .unwrap_or(false);
        if !ours {
            continue;
        }
        match link::remove_symlink(&path) {
            Ok(()) => {
                r.line(format!("removed {}", rel_display(env, &path)));
                removed += 1;
            }
            Err(e) => r.problem(format!("cannot remove {}: {e}", path.display())),
        }
    }
    removed
}

/// Hook elements of ours, by element identity (ADR-0003).
fn revert_hooks(env: &Env, target: &Target, commons: &Commons, r: &mut Reporter) -> usize {
    let Some(agent) = target.agent else {
        return 0;
    };
    let Hooks::KeyMerge { file, root_key } = agent.hooks else {
        return 0;
    };

    let path = env.in_home(file);
    let strip = match hooks::strip(
        &path,
        root_key,
        &target.name,
        &commons.family_dir(commons::HOOKS),
    ) {
        Ok(strip) => strip,
        Err(e) => {
            r.problem(e.to_string());
            return 0;
        }
    };

    for problem in &strip.problems {
        r.problem(problem);
    }
    for hook in &strip.removed {
        r.line(format!(
            "removed the {} hook {} from {}",
            hook.event,
            hook.command,
            rel_display(env, &path)
        ));
    }
    strip.removed.len()
}

/// Remove every symlink of ours directly inside one fan-out directory. The
/// directory itself stays — agent-sync never deletes an agent's directories.
fn remove_links(env: &Env, dir: &Path, commons: &Path, r: &mut Reporter) -> usize {
    let mut removed = 0usize;
    for path in entries_of(dir) {
        removed += remove_our_link(env, &path, commons, r);
    }
    removed
}

/// Remove the object at `path` if it is a symlink of ours; anything else stays.
fn remove_our_link(env: &Env, path: &Path, commons: &Path, r: &mut Reporter) -> usize {
    if !link::is_ours(path, commons) {
        return 0;
    }
    match link::remove_symlink(path) {
        Ok(()) => {
            r.line(format!("removed {}", rel_display(env, path)));
            1
        }
        Err(e) => {
            r.problem(format!("cannot remove {}: {e}", path.display()));
            0
        }
    }
}

/// A directory's entries in a stable order; an absent directory is empty.
fn entries_of(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    paths
}

/// A path the way reports show it: home-relative where possible.
fn rel_display(env: &Env, path: &Path) -> String {
    path.strip_prefix(env.home())
        .unwrap_or(path)
        .display()
        .to_string()
}
