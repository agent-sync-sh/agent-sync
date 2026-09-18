//! `sync` — make every Target match the Commons.
//!
//! Two phases (ADR-0007). Phase 1 surveys every family read-only and collects
//! **every** problem; if any exist they are all reported and the run exits
//! having written nothing, so the user fixes one complete list rather than one
//! error per run. Phase 2 executes the plan; a write failure there is reported
//! and the remaining operations continue, because everything was resolved
//! before the first write and an idempotent re-run converges. `--dry-run` is
//! phase 1 plus the printed plan — one code path, so the preview cannot drift
//! from what a real run would do.

use std::path::Path;

use crate::commons::{self, Commons};
use crate::config::Config;
use crate::env::Env;
use crate::family::Family;
use crate::hooks;
use crate::instructions;
use crate::link::{self, Item};
use crate::lock;
use crate::mcp;
use crate::render;
use crate::report::Reporter;
use crate::target;
use crate::{EXIT_CLEAN, EXIT_ERROR};

/// Everything one run intends to do, surveyed and resolved before any write.
struct Plan {
    /// Advisory Commons scan issues, in family order. Never block the run.
    warnings: Vec<String>,
    /// Symlink-family surveys, in family then Target order.
    links: Vec<LinkPlan>,
    instructions: Vec<instructions::Item>,
    mcp: mcp::Survey,
    rendered: Vec<render::Item>,
    hooks: hooks::Survey,
    /// Blocking survey problems, collected across every family. Any entry:
    /// phase 2 never runs.
    problems: Vec<String>,
}

/// One Target directory's worth of symlink work.
struct LinkPlan {
    agent: String,
    dir: String,
    items: Vec<Item>,
}

impl Plan {
    /// Phase 1: survey every family, read-only, collecting every problem.
    fn build(env: &Env, config: &Config, commons: &Commons) -> Plan {
        let targets = target::resolve(env, config);
        let mut warnings = Vec::new();
        let mut links = Vec::new();

        for family in Family::ALL {
            let scan = commons.scan(*family);
            warnings.extend(scan.issues.iter().map(ToString::to_string));

            for target in &targets {
                if let Some(dir) = target.fanout_dir(*family) {
                    let items = link::survey(&env.in_home(dir), commons.root(), &scan.entries);
                    links.push(LinkPlan {
                        agent: target.name.clone(),
                        dir: dir.to_string(),
                        items,
                    });
                } else if let Some(dir) = target.legacy_dir(*family) {
                    // A flipped agent's old fan-out dir: the agent reads it
                    // *and* the Commons, so our links there are live
                    // duplicates. Prune-only — remove ours, create nothing,
                    // touch nothing foreign.
                    let items = link::survey_legacy(&env.in_home(dir), commons.root());
                    links.push(LinkPlan {
                        agent: target.name.clone(),
                        dir: dir.to_string(),
                        items,
                    });
                }
            }
        }

        let instructions =
            instructions::survey(env, config, &commons.root().join(commons::INSTRUCTIONS));
        let mut mcp = mcp::survey(env, config, &commons.root().join(commons::MCP));
        let mut rendered = render::survey(env, config, commons);
        let mut hooks = hooks::survey(env, config, &commons.family_dir(commons::HOOKS));

        let mut problems = Vec::new();
        problems.append(&mut mcp.problems);
        problems.append(&mut rendered.problems);
        problems.append(&mut hooks.problems);

        Plan {
            warnings,
            links,
            instructions,
            mcp,
            rendered: rendered.items,
            hooks,
            problems,
        }
    }
}

pub fn run(env: &Env, config: &Config, r: &mut Reporter, dry_run: bool) -> i32 {
    let commons = Commons::new(env.commons());
    if !commons.exists() {
        r.problem(commons::missing_message(commons.root()));
        return EXIT_ERROR;
    }

    // A dry run only reads, so it never contends for the lock.
    let _lock = if dry_run {
        None
    } else {
        match lock::acquire(env.state_dir(), lock::timeout(env)) {
            Ok(guard) => Some(guard),
            Err(e) => {
                r.problem(e.to_string());
                return EXIT_ERROR;
            }
        }
    };

    if dry_run {
        r.line("dry run — no changes will be made");
        r.blank();
    }

    // Phase 1 — survey everything before the first write.
    let plan = Plan::build(env, config, &commons);
    for warning in &plan.warnings {
        r.warn(warning);
    }

    let mut changes = 0usize;
    if plan.problems.is_empty() {
        // Phase 2 — execute the plan (a dry run prints it instead). Hooks are
        // written after MCP because Gemini keeps both in one settings file,
        // and each family re-reads it before merging.
        for link_plan in &plan.links {
            changes += sync_target(
                &link_plan.agent,
                &link_plan.dir,
                &link_plan.items,
                env.home(),
                r,
                dry_run,
            );
        }
        changes += sync_instructions(&plan.instructions, r, dry_run);
        changes += sync_mcp(&plan.mcp, r, dry_run);
        changes += sync_rendered(&plan.rendered, r, dry_run);
        changes += sync_hooks(&plan.hooks, r, dry_run);
    } else {
        // A survey problem anywhere stops the whole run before its first
        // write, so a config error can never leave a half-synced machine.
        for problem in &plan.problems {
            r.problem(problem);
        }
        // Skipped Targets are faults too; name every problem in the same run.
        for skipped in plan.mcp.skipped.iter().chain(&plan.hooks.skipped) {
            r.problem(skipped);
        }
    }

    if changes == 0 && r.problem_count() == 0 {
        r.line("Everything is up to date.");
    } else if changes == 0 {
        r.blank();
        r.line("No changes were made — see the errors above.");
    } else {
        r.blank();
        let noun = if changes == 1 { "change" } else { "changes" };
        let verb = if dry_run { "would be made" } else { "made" };
        r.line(format!("{changes} {noun} {verb}."));
    }

    if r.problem_count() > 0 {
        EXIT_ERROR
    } else {
        EXIT_CLEAN
    }
}

/// The rendered family: whole files generated for agents that cannot take a
/// symlink, each carrying the Marker that makes it ours to replace.
fn sync_rendered(items: &[render::Item], r: &mut Reporter, dry_run: bool) -> usize {
    let noteworthy: Vec<&render::Item> = items
        .iter()
        .filter(|i| i.state != render::State::Managed)
        .collect();
    if noteworthy.is_empty() {
        return 0;
    }

    r.line("rendered commands");
    for item in &noteworthy {
        r.line(format!(
            "  {:<10} {} → {} — {}",
            item.state.label(),
            item.name,
            item.target,
            item.state.note()
        ));
    }

    let changing: Vec<&&render::Item> = noteworthy
        .iter()
        .filter(|i| i.state.needs_change())
        .collect();
    if changing.is_empty() || dry_run {
        return changing.len();
    }

    let mut written = 0usize;
    for item in &changing {
        match render::apply(item) {
            Ok(()) => written += 1,
            Err(e) => r.problem(e.to_string()),
        }
    }
    written
}

/// The hooks family: command-hooks merged into each agent's hook arrays.
fn sync_hooks(survey: &hooks::Survey, r: &mut Reporter, dry_run: bool) -> usize {
    for skipped in &survey.skipped {
        r.problem(skipped);
    }

    let noteworthy: Vec<&hooks::Item> = survey
        .items
        .iter()
        .filter(|i| i.state != hooks::State::Managed)
        .collect();
    if noteworthy.is_empty() {
        return 0;
    }

    r.line("hooks");
    for item in &noteworthy {
        r.line(format!(
            "  {:<12} {} {} → {} — {}",
            item.state.label(),
            item.event,
            short(&item.label),
            item.target,
            item.note()
        ));
    }

    let changing: Vec<&hooks::Item> = noteworthy
        .into_iter()
        .filter(|i| i.state.needs_change())
        .collect();
    if changing.is_empty() || dry_run {
        return changing.len();
    }

    let mut written = 0usize;
    for (path, group) in by_file(&changing) {
        match hooks::apply(path, group[0].root_key, &group) {
            Ok(report) => {
                written += group.len();
                if report.exposed {
                    r.warn(format!(
                        "{} has mode {:o} — it now holds resolved secrets and can be read by others",
                        report.path.display(),
                        report.mode
                    ));
                }
            }
            Err(e) => r.problem(e.to_string()),
        }
    }

    written
}

/// A command, shortened for a report line. Never resolved.
fn short(command: &str) -> String {
    const MAX: usize = 40;
    if command.chars().count() <= MAX {
        return command.to_string();
    }
    let head: String = command.chars().take(MAX - 1).collect();
    format!("{head}…")
}

/// The MCP family: one Commons file, rendered and key-merged per agent.
fn sync_mcp(survey: &mcp::Survey, r: &mut Reporter, dry_run: bool) -> usize {
    // A config agent-sync cannot parse is a real fault worth an exit code, but
    // it must not stop the Targets that are healthy.
    for skipped in &survey.skipped {
        r.problem(skipped);
    }
    let noteworthy: Vec<&mcp::Item> = survey
        .items
        .iter()
        .filter(|i| i.state != mcp::State::Managed)
        .collect();
    if noteworthy.is_empty() {
        return 0;
    }

    r.line("mcp (mcp.json)");
    for item in &noteworthy {
        r.line(format!(
            "  {:<9} {} → {} — {}",
            item.state.label(),
            item.name,
            item.target,
            item.note()
        ));
    }

    let changing: Vec<&mcp::Item> = noteworthy
        .into_iter()
        .filter(|i| i.state.needs_change())
        .collect();
    if changing.is_empty() || dry_run {
        return changing.len();
    }

    // A Disabled server's leftover render leaves through the one deletion
    // primitive: its name is still in the Commons, so the removal rests on the
    // same name identity as the write that put it there.
    let (removals, writes): (Vec<&mcp::Item>, Vec<&mcp::Item>) = changing
        .into_iter()
        .partition(|i| i.state == mcp::State::Disabled);

    // One agent's write failing must not cancel the others: everything was
    // already resolved before any file was opened, so the remaining writes are
    // still correct. Each failure is reported and the run ends non-clean.
    let mut written = 0usize;
    for item in &removals {
        match mcp::strip(&item.path, item.root_key, item.format, &item.name) {
            Ok(_) => written += 1,
            Err(e) => r.problem(e.to_string()),
        }
    }
    for (path, group) in by_file(&writes) {
        let root_key = group[0].root_key;
        match mcp::apply(path, root_key, group[0].format, &group) {
            Ok(report) => {
                written += group.len();
                if report.exposed {
                    r.warn(format!(
                        "{} has mode {:o} — it now holds resolved secrets and can be read by others",
                        report.path.display(),
                        report.mode
                    ));
                }
            }
            Err(e) => r.problem(e.to_string()),
        }
    }

    written
}

/// A trait for items that know which config file they belong to, so shared
/// files receive one coherent write.
trait InFile {
    fn path(&self) -> &Path;
}

impl InFile for mcp::Item {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl InFile for hooks::Item {
    fn path(&self) -> &Path {
        &self.path
    }
}

/// Group items by the config file they land in, preserving order. One write
/// per file, not per item: an agent may take more than one entry — or more
/// than one family — in the same file, and a write per item would clobber the
/// previous one.
fn by_file<'i, T: InFile>(items: &[&'i T]) -> Vec<(&'i Path, Vec<&'i T>)> {
    let mut grouped: Vec<(&Path, Vec<&T>)> = Vec::new();
    for item in items.iter().copied() {
        match grouped.iter_mut().find(|(path, _)| *path == item.path()) {
            Some((_, group)) => group.push(item),
            None => grouped.push((item.path(), vec![item])),
        }
    }
    grouped
}

/// The instructions family: one Commons file, four per-agent mechanisms.
fn sync_instructions(items: &[instructions::Item], r: &mut Reporter, dry_run: bool) -> usize {
    let noteworthy: Vec<&instructions::Item> = items
        .iter()
        .filter(|i| i.state != instructions::State::Linked)
        .filter(|i| i.state != instructions::State::ImportPresent)
        .collect();
    if noteworthy.is_empty() {
        return 0;
    }

    r.line("instructions (AGENTS.md)");
    let mut changed = 0;

    for item in noteworthy {
        if !item.state.needs_change() {
            r.line(format!(
                "  {:<9} {} — {}",
                item.state.label(),
                item.target,
                item.note()
            ));
            continue;
        }
        if dry_run {
            r.line(format!("  {:<9} {}", item.state.label(), item.target));
            changed += 1;
        } else if let Err(e) = instructions::apply(item) {
            r.problem(format!("cannot write {}: {e}", item.path.display()));
        } else {
            r.line(format!("  {:<9} {}", item.state.label(), item.target));
            changed += 1;
        }
    }

    changed
}

/// Report — and unless this is a dry run, apply — one Target's items. Returns
/// how many of them changed the filesystem.
fn sync_target(
    agent: &str,
    dir: &str,
    items: &[Item],
    home: &Path,
    r: &mut Reporter,
    dry_run: bool,
) -> usize {
    let interesting: Vec<&Item> = items
        .iter()
        .filter(|i| i.state != link::State::Linked)
        .collect();
    if interesting.is_empty() {
        return 0;
    }

    r.line(format!("{agent} ({dir})"));
    let mut changed = 0;

    for item in interesting {
        let note = item.state.note();
        if !item.state.needs_change() {
            r.line(format!(
                "  {:<9} {} — {note}",
                item.state.label(),
                item.name
            ));
            continue;
        }

        if dry_run {
            r.line(format!("  {:<9} {}", item.state.label(), item.name));
            changed += 1;
        } else if let Err(e) = link::apply(item) {
            r.problem(format!(
                "cannot fix {}: {e}",
                item.path.strip_prefix(home).unwrap_or(&item.path).display()
            ));
        } else {
            r.line(format!("  {:<9} {}", item.state.label(), item.name));
            changed += 1;
        }
    }

    changed
}
