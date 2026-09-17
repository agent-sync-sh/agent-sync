//! `init` — create the Commons skeleton, then say what this machine already has.
//!
//! Scaffolding is deliberately minimal: the Commons and its fan-out family
//! directories, nothing else. It never creates an agent root, because a root's
//! existence is what detection means; it never invents an `AGENTS.md`, because
//! an empty one would fan out to every agent as empty instructions; and it
//! never creates `hooks/`, because an empty one would mean "this family is in
//! use" and start reporting every tool's own hooks.
//!
//! The report that follows is the on-ramp. On a machine that has been used, the
//! interesting question is not "what does agent-sync do" but "what of mine could
//! it take over", so the report answers that, in the project's vocabulary, with
//! the command that would do it.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::commons::{self, Commons};
use crate::config::Config;
use crate::env::Env;
use crate::family::Family;
use crate::instructions;
use crate::mcp;
use crate::registry::Mcp;
use crate::report::Reporter;
use crate::target::{self, Target};
use crate::{EXIT_CLEAN, EXIT_ERROR};

pub fn run(env: &Env, config: &Config, r: &mut Reporter) -> i32 {
    let commons = Commons::new(env.commons());
    let existed = commons.exists();

    for dir in std::iter::once(commons.root().to_path_buf())
        .chain(Family::ALL.iter().map(|f| commons.family_dir(f.name())))
    {
        if let Err(e) = fs::create_dir_all(&dir) {
            r.problem(format!("cannot create {}: {e}", dir.display()));
            return EXIT_ERROR;
        }
    }

    if existed {
        r.line(format!(
            "Commons already present at {}",
            commons.root().display()
        ));
    } else {
        r.line(format!(
            "Created the Commons at {}",
            commons.root().display()
        ));
    }
    for family in Family::ALL {
        r.line(format!("  {}/", family.name()));
    }
    r.blank();

    let targets = target::resolve(env, config);
    report_agents(&targets, r);
    let offered = report_candidates(env, &commons, &targets, r);
    report_conflicts(env, config, &commons, r);

    r.blank();
    if offered {
        r.line("Adopt what you want to share, then run `agent-sync sync`.");
    } else {
        r.line("Put a skill in skills/, instructions in AGENTS.md, then run `agent-sync sync`.");
    }

    EXIT_CLEAN
}

fn report_agents(targets: &[Target], r: &mut Reporter) {
    if targets.is_empty() {
        r.line("No agents found — agent-sync syncs to agents that are already installed.");
        return;
    }
    let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
    r.line(format!(
        "Found {} agents: {}",
        names.len(),
        names.join(", ")
    ));
}

/// What is on this machine that the Commons could take over. Returns whether
/// anything was offered.
fn report_candidates(env: &Env, commons: &Commons, targets: &[Target], r: &mut Reporter) -> bool {
    let mut offered = false;

    for family in Family::ALL {
        let held: BTreeSet<String> = commons
            .scan(*family)
            .entries
            .into_iter()
            .map(|e| e.file_name)
            .collect();

        let mut found: Vec<String> = Vec::new();
        for target in targets {
            let Some(dir) = target.fanout_dir(*family) else {
                continue;
            };
            for name in real_entries(&env.in_home(dir)) {
                if held.contains(&name) {
                    continue;
                }
                found.push(format!("{}/{name}", dir.trim_start_matches('.')));
            }
        }

        if found.is_empty() {
            continue;
        }
        offered = true;
        r.blank();
        r.line(format!(
            "{} {} not in the Commons:",
            found.len(),
            family.name()
        ));
        for path in found.iter().take(8) {
            r.line(format!("  {path}"));
        }
        if found.len() > 8 {
            r.line(format!("  … and {} more", found.len() - 8));
        }
        r.line("  take one with `agent-sync adopt <path>`");
    }

    // MCP servers are named rather than pathed, and adopt takes them in bulk.
    let known: BTreeSet<String> = mcp::commons_servers(&commons.root().join(commons::MCP))
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default();
    let mut servers: BTreeSet<String> = BTreeSet::new();
    for target in targets {
        let Some(agent) = target.agent else {
            continue;
        };
        let Mcp::KeyMerge {
            file,
            root_key,
            format,
            ..
        } = agent.mcp
        else {
            continue;
        };
        // A config we cannot read is not a candidate; `doctor` says so already.
        if let Ok(found) = mcp::native_servers(&env.in_home(file), root_key, format) {
            for name in found.keys() {
                if !known.contains(name) {
                    servers.insert(name.clone());
                }
            }
        }
    }

    if !servers.is_empty() {
        offered = true;
        let names: Vec<&str> = servers.iter().map(String::as_str).collect();
        r.blank();
        r.line(format!(
            "{} MCP servers not in the Commons: {}",
            names.len(),
            names.join(", ")
        ));
        r.line("  take them all with `agent-sync mcp adopt --all`");
    }

    if !offered {
        r.blank();
        r.line("Nothing to adopt — no agent config here is outside the Commons.");
    }
    offered
}

/// Instructions files another tool already owns, which agent-sync will not touch.
fn report_conflicts(env: &Env, config: &Config, commons: &Commons, r: &mut Reporter) {
    let items = instructions::survey(env, config, &commons.root().join(commons::INSTRUCTIONS));
    let conflicts: Vec<&instructions::Item> = items
        .iter()
        .filter(|i| i.state == instructions::State::Conflict)
        .collect();
    if conflicts.is_empty() {
        return;
    }

    r.blank();
    r.line(format!("{} conflicts:", conflicts.len()));
    for item in conflicts {
        r.line(format!("  {} — {}", item.target, item.note()));
    }
}

/// Names in a Target directory that are real objects rather than links: the
/// things `adopt` could take. A link is either already ours or Foreign, and
/// neither is a candidate.
fn real_entries(dir: &Path) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .flatten()
        .filter(|entry| {
            fs::symlink_metadata(entry.path())
                .map(|meta| !meta.file_type().is_symlink())
                .unwrap_or(false)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}
