//! `adopt` — one verb; where the path lives picks the mechanic (ADR-0006).
//!
//! Three mechanics. **Absorb**: a real file or directory in a Target surface
//! moves into the Commons and leaves a relative link behind. **Source**: a
//! path whose home is a git repo — a `.git` directory or file found walking
//! up — stays where it is, and the Commons gains an absolute symlink to it,
//! the path exactly as given and never resolved through. **Copy**: everything
//! else has no durable home worth pointing at — the content is copied in, the
//! original stays, and nothing points back. Every mechanic ends by fanning
//! the entry out to every installed agent through the same machinery `sync`
//! uses — so a sync immediately after has nothing to say.
//!
//! No `--force`, ever. Refusing to merge a divergence by hand is the point:
//! silently discarding one side of it is exactly the failure mode this tool is
//! defined against.

use std::fs;
use std::path::{Path, PathBuf};

use crate::commons::{self, Commons, Entry};
use crate::config::Config;
use crate::env::Env;
use crate::family::{Family, Shape};
use crate::instructions;
use crate::link;
use crate::lock;
use crate::registry::Instructions;
use crate::report::Reporter;
use crate::target;
use crate::{EXIT_CLEAN, EXIT_ERROR};

pub fn run(env: &Env, config: &Config, r: &mut Reporter, raw_path: &str, dry_run: bool) -> i32 {
    let commons = Commons::new(env.commons());
    if !commons.exists() {
        r.problem(commons::missing_message(commons.root()));
        return EXIT_ERROR;
    }

    let path = absolute(raw_path);

    if !path.exists() && fs::symlink_metadata(&path).is_err() {
        r.problem(format!("{} does not exist", path.display()));
        return EXIT_ERROR;
    }

    // The Commons guard, ahead of every other classification — including the
    // symlink refusal. The Commons is meant to be committed, and once the git
    // walk-up lands, a `.git` at its root would classify every entry as
    // repo-backed: adopt would replace the entry with a link to itself,
    // destroying it. Nothing inside the Commons is ever adoptable.
    if path.starts_with(link::normalize(env.commons())) {
        r.problem(format!(
            "{} is already in the Commons — nothing to adopt",
            path.display()
        ));
        return EXIT_ERROR;
    }

    // Which Target surface is this, and therefore where does it belong? A
    // declared surface always wins — even inside a git repo, because a custom
    // target declaration is an explicit statement that the dir has fan-out
    // semantics (ADR-0006). Only a path no surface claims gets the walk-up.
    match locate(env, config, &path) {
        Some(placement) => absorb(env, config, &commons, &path, &placement, r, dry_run),
        // A `.git` above the path — directory or file, covering worktrees and
        // submodules — means the content has a durable home: Source it.
        None if in_git_repo(&path) => source(env, config, &commons, &path, r, dry_run),
        // No surface, no repo above: no durable home worth pointing at. Copy.
        None => copy(env, config, &commons, &path, r, dry_run),
    }
}

/// The absorb mechanic: move a Target-surface path into the Commons, leave a
/// relative link behind, fan out.
fn absorb(
    env: &Env,
    config: &Config,
    commons: &Commons,
    path: &Path,
    placement: &Placement,
    r: &mut Reporter,
    dry_run: bool,
) -> i32 {
    // A symlink in a Target surface is already ours or Foreign — nothing to
    // absorb. This refusal belongs to the absorb branch alone: a symlink
    // *input* is legal for the Source mechanic, which links to it as given.
    if fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        r.problem(format!(
            "{} is already a symlink — there is nothing to adopt",
            path.display()
        ));
        return EXIT_ERROR;
    }

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let destination = commons.root().join(&placement.commons_rel);

    // A Commons symlink under this name belongs to the unified matrix, and it
    // must answer before any content comparison: relinking through a link that
    // resolves right back here would destroy the only real copy.
    if fs::symlink_metadata(&destination)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        let entry = match placement.surface {
            Surface::FanOut(family) => format!("{}/{file_name}", family.name()),
            Surface::Instructions => commons::INSTRUCTIONS.to_string(),
        };
        return existing_link_answer(&entry, &destination, path, r);
    }

    // Everything is decided before the lock is taken, so a refusal leaves the
    // machine exactly as it was — not even a lock file.
    if destination.exists() && !link::same_contents(path, &destination) {
        r.problem(divergence_message(path, &destination));
        return EXIT_ERROR;
    }

    // The Commons already holding an identical copy means relink, not move.
    let identical = destination.exists();

    let adoption = Adoption {
        env,
        config,
        commons,
        surface: placement.surface,
        origin: path,
        destination: &destination,
    };

    // A dry run classifies exactly like a real run — every refusal above
    // already answered in the same words with the same exit code — and stops
    // here: read-only, lockless, the mechanic named, every action in "would"
    // tense.
    if dry_run {
        r.line("dry run — no changes will be made");
        r.blank();
        r.line(format!(
            "absorb: {file_name} from {} into the Commons ({})",
            placement.target,
            placement.family_label()
        ));
        if identical {
            r.line(format!(
                "  would remove {} — identical to the Commons copy",
                path.display()
            ));
        } else {
            r.line(format!("  would move {} into the Commons", path.display()));
        }
        r.line(format!(
            "  would leave a link at {} → {}",
            path.display(),
            canonical_text(path, &destination).display()
        ));
        fan_out(&adoption, r, true);
        return EXIT_CLEAN;
    }

    let _lock = match lock::acquire(env.state_dir(), lock::timeout(env)) {
        Ok(guard) => guard,
        Err(e) => {
            r.problem(e.to_string());
            return EXIT_ERROR;
        }
    };

    let code = if identical {
        relink(path, &destination, &file_name, &placement.target, r)
    } else {
        move_in(path, &destination, &file_name, placement, r)
    };
    if code != EXIT_CLEAN {
        return code;
    }

    // Absorb finishes the job: the entry reaches every installed agent now,
    // so a sync immediately after has nothing to do.
    fan_out(&adoption, r, false);
    r.verdict()
}

/// The Commons does not have this name: move it in, leave a link behind.
fn move_in(
    path: &Path,
    destination: &Path,
    file_name: &str,
    placement: &Placement,
    r: &mut Reporter,
) -> i32 {
    if let Some(parent) = destination.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        r.problem(format!("cannot create {}: {e}", parent.display()));
        return EXIT_ERROR;
    }

    if let Err(e) = move_path(path, destination) {
        r.problem(format!(
            "cannot move {} into the Commons: {e}",
            path.display()
        ));
        return EXIT_ERROR;
    }

    match place_link(path, destination) {
        Ok(text) => {
            r.line(format!(
                "adopted {file_name} from {} into the Commons ({})",
                placement.target,
                placement.family_label()
            ));
            r.line(format!("  {} → {}", path.display(), text.display()));
            EXIT_CLEAN
        }
        Err(e) => {
            // Put it back rather than leave the user with neither copy.
            let _ = move_path(destination, path);
            r.problem(format!("cannot link {}: {e}", path.display()));
            EXIT_ERROR
        }
    }
}

/// The Commons already has an identical copy: drop the duplicate, link instead.
fn relink(path: &Path, destination: &Path, file_name: &str, target: &str, r: &mut Reporter) -> i32 {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(e) = removed {
        r.problem(format!("cannot remove {}: {e}", path.display()));
        return EXIT_ERROR;
    }

    match place_link(path, destination) {
        Ok(text) => {
            r.line(format!(
                "{file_name} in {target} was identical to the Commons copy — re-linked"
            ));
            r.line(format!("  {} → {}", path.display(), text.display()));
            EXIT_CLEAN
        }
        Err(e) => {
            r.problem(format!("cannot link {}: {e}", path.display()));
            EXIT_ERROR
        }
    }
}

/// The Source mechanic: the path has a durable home in a git repo, so it stays
/// there and the Commons gains an **absolute** symlink to it — the path exactly
/// as given, lexically normalized but never resolved through, so a symlink
/// input is linked *to*, not through (ADR-0006).
fn source(
    env: &Env,
    config: &Config,
    commons: &Commons,
    path: &Path,
    r: &mut Reporter,
    dry_run: bool,
) -> i32 {
    let Some(family) = family_from_parent(path, r) else {
        return EXIT_ERROR;
    };

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let destination = commons.root().join(family.name()).join(&file_name);
    let entry = format!("{}/{file_name}", family.name());

    // The collision matrix, decided before the lock — a refusal (and the
    // idempotent no-op) leaves the machine exactly as it was.
    let mut replace = false;
    if let Ok(meta) = fs::symlink_metadata(&destination) {
        if meta.file_type().is_symlink() {
            return existing_link_answer(&entry, &destination, path, r);
        }
        if !link::same_contents(path, &destination) {
            r.problem(divergence_message(path, &destination));
            return EXIT_ERROR;
        }
        // The Commons copy is identical: the link replaces it, and the Source
        // keeps the single real copy from here on.
        replace = true;
    }

    let adoption = Adoption {
        env,
        config,
        commons,
        surface: Surface::FanOut(family),
        origin: path,
        destination: &destination,
    };

    if dry_run {
        r.line("dry run — no changes will be made");
        r.blank();
        r.line(format!(
            "source: {file_name} — Sourced from {} ({})",
            path.display(),
            family.name()
        ));
        if replace {
            r.line("  would replace the identical Commons copy with the link");
        }
        r.line(format!(
            "  would link {} → {}",
            destination.display(),
            path.display()
        ));
        fan_out(&adoption, r, true);
        return EXIT_CLEAN;
    }

    let _lock = match lock::acquire(env.state_dir(), lock::timeout(env)) {
        Ok(guard) => guard,
        Err(e) => {
            r.problem(e.to_string());
            return EXIT_ERROR;
        }
    };

    if replace {
        let removed = if destination.is_dir() {
            fs::remove_dir_all(&destination)
        } else {
            fs::remove_file(&destination)
        };
        if let Err(e) = removed {
            r.problem(format!("cannot remove {}: {e}", destination.display()));
            return EXIT_ERROR;
        }
    } else if let Some(parent) = destination.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        r.problem(format!("cannot create {}: {e}", parent.display()));
        return EXIT_ERROR;
    }

    if let Err(e) = link::create_symlink(path, &destination) {
        r.problem(format!("cannot link {}: {e}", destination.display()));
        return EXIT_ERROR;
    }

    r.line(format!(
        "adopted {file_name} — Sourced from {} ({})",
        path.display(),
        family.name()
    ));
    if replace {
        r.line("  replaced the identical Commons copy with the link");
    }
    r.line(format!("  {} → {}", destination.display(), path.display()));

    fan_out(&adoption, r, false);
    r.verdict()
}

/// The Copy mechanic: an external path with no durable home is copied into
/// the Commons — a real copy, not a move. The original stays, nothing points
/// back, and the message says so out loud: a deliberate divorce, not a
/// managed relationship (ADR-0006). A symlink input is copied *through* — the
/// content it resolves to, `fs::copy` semantics.
fn copy(
    env: &Env,
    config: &Config,
    commons: &Commons,
    path: &Path,
    r: &mut Reporter,
    dry_run: bool,
) -> i32 {
    let Some(family) = family_from_parent(path, r) else {
        return EXIT_ERROR;
    };

    // The family fixes the shape. locate() implied it for surface paths; an
    // external path may hand us either, so a mismatch refuses here.
    let complaint = match family.shape() {
        Shape::Directory if !path.is_dir() => Some("entries are directories — this is a file"),
        Shape::Markdown if path.is_dir() => {
            Some("entries are single .md files — this is a directory")
        }
        _ => None,
    };
    if let Some(complaint) = complaint {
        r.problem(format!("{}: {} {complaint}", path.display(), family.name()));
        return EXIT_ERROR;
    }

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let destination = commons.root().join(family.name()).join(&file_name);
    let entry = format!("{}/{file_name}", family.name());

    // The collision matrix, decided before the lock. The identical case is a
    // clean no-op: the original stays too — nothing was ever going to point
    // back at it, so there is nothing to delete.
    if let Ok(meta) = fs::symlink_metadata(&destination) {
        if meta.file_type().is_symlink() {
            return existing_link_answer(&entry, &destination, path, r);
        }
        if !link::same_contents(path, &destination) {
            r.problem(divergence_message(path, &destination));
            return EXIT_ERROR;
        }
        r.line(format!("{entry} is already in the Commons — nothing to do"));
        return EXIT_CLEAN;
    }

    let adoption = Adoption {
        env,
        config,
        commons,
        surface: Surface::FanOut(family),
        origin: path,
        destination: &destination,
    };

    if dry_run {
        r.line("dry run — no changes will be made");
        r.blank();
        r.line(format!(
            "copy: {file_name} — copied into the Commons ({})",
            family.name()
        ));
        r.line(format!("  would copy {} into the Commons", path.display()));
        r.line(divorce_line(path));
        fan_out(&adoption, r, true);
        return EXIT_CLEAN;
    }

    let _lock = match lock::acquire(env.state_dir(), lock::timeout(env)) {
        Ok(guard) => guard,
        Err(e) => {
            r.problem(e.to_string());
            return EXIT_ERROR;
        }
    };

    if let Some(parent) = destination.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        r.problem(format!("cannot create {}: {e}", parent.display()));
        return EXIT_ERROR;
    }

    if let Err(e) = copy_tree(path, &destination) {
        // Leave no half-copied entry behind for sync to fan out.
        let _ = if destination.is_dir() {
            fs::remove_dir_all(&destination)
        } else {
            fs::remove_file(&destination)
        };
        r.problem(format!(
            "cannot copy {} into the Commons: {e}",
            path.display()
        ));
        return EXIT_ERROR;
    }

    r.line(format!(
        "adopted {file_name} — copied into the Commons ({})",
        family.name()
    ));
    r.line(divorce_line(path));

    fan_out(&adoption, r, false);
    r.verdict()
}

/// The one loud sentence Copy pays for future divergence with — present tense
/// in real and dry runs alike, because it states a fact, not an action.
fn divorce_line(path: &Path) -> String {
    format!(
        "  the original at {} is no longer consulted; edits there will \
         diverge from the Commons copy",
        path.display()
    )
}

/// Whether a `.git` — directory or file, covering worktrees and submodules —
/// sits at the path or anywhere above it: the content has a durable home.
fn in_git_repo(path: &Path) -> bool {
    path.ancestors()
        .any(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
}

/// Whether two paths name the same place. Lexical equality answers first — a
/// dangling path still compares — and the filesystem's own resolution answers
/// for two spellings of one file, so an absolute link reaching the same path
/// by another route still matches.
fn same_place(a: &Path, b: &Path) -> bool {
    if link::normalize(a) == link::normalize(b) {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// One answer for a Commons symlink already holding an entry's name, shared
/// by every mechanic so the matrix reads the same everywhere: a link resolving
/// to this very path is the idempotent no-op, a link elsewhere refuses and
/// names the fix.
fn existing_link_answer(entry: &str, destination: &Path, path: &Path, r: &mut Reporter) -> i32 {
    let text = fs::read_link(destination).unwrap_or_default();
    let resolved = link::resolve_link(destination, &text);
    if same_place(&resolved, path) {
        r.line(format!(
            "{entry} is already Sourced from {} — nothing to do",
            path.display()
        ));
        return EXIT_CLEAN;
    }
    r.problem(format!(
        "{entry} is already Sourced from {} — remove the Commons link \
         first if you mean to repoint it",
        resolved.display()
    ));
    EXIT_ERROR
}

/// The family an external path belongs to — its parent directory's basename —
/// or the refusal naming the accepted ones. Shared by Source and Copy: the
/// non-family answer reads the same whichever mechanic asked.
fn family_from_parent(path: &Path, r: &mut Reporter) -> Option<Family> {
    let parent_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let family = Family::ALL
        .iter()
        .copied()
        .find(|f| f.name() == parent_name);
    if family.is_none() {
        r.problem(format!(
            "{}: parent directory \"{parent_name}\" is not a family — \
             adopt takes paths from {}",
            path.display(),
            family_list()
        ));
    }
    family
}

/// One divergence refusal for every mechanic: agent-sync never chooses which
/// side of a Variant to discard.
fn divergence_message(path: &Path, destination: &Path) -> String {
    format!(
        "{} differs from the Commons copy at {} — this is a Variant. \
         Merge it by hand if you want it in the Commons; agent-sync will not \
         choose which side to discard",
        path.display(),
        destination.display()
    )
}

/// The family directories an external path may come from, in report order:
/// `skills/, commands/, or agents/`.
fn family_list() -> String {
    let names: Vec<String> = Family::ALL
        .iter()
        .map(|f| format!("{}/", f.name()))
        .collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// One adoption, resolved: everything the fan-out needs to know.
struct Adoption<'a> {
    env: &'a Env,
    config: &'a Config,
    commons: &'a Commons,
    surface: Surface,
    /// The adopted path, normalized. It already holds its link (or, in a dry
    /// run, still holds the content), so the fan-out passes it by.
    origin: &'a Path,
    /// The Commons entry the adoption produces.
    destination: &'a Path,
}

/// Fan the adopted entry out to every installed agent, through the same
/// survey/apply machinery `sync` runs per entry — Variants kept, Native agents
/// skipped. A dry run prints the same items in "would" tense and writes
/// nothing; a write failure is reported and the remaining agents still get
/// their links, exactly as a sync write failure behaves.
fn fan_out(a: &Adoption, r: &mut Reporter, dry_run: bool) {
    match a.surface {
        Surface::FanOut(family) => fan_out_family(a, family, r, dry_run),
        Surface::Instructions => fan_out_instructions(a, r, dry_run),
    }
}

/// One entry of one symlink family, surveyed and applied per Target.
fn fan_out_family(a: &Adoption, family: Family, r: &mut Reporter, dry_run: bool) {
    let file_name = a
        .destination
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match family.shape() {
        Shape::Markdown => file_name.trim_end_matches(".md").to_string(),
        Shape::Directory => file_name.clone(),
    };
    let entry = Entry {
        name,
        file_name,
        path: a.destination.to_path_buf(),
    };

    for target in target::resolve(a.env, a.config) {
        // Native agents and absent surfaces alike take no fan-out directory.
        let Some(dir) = target.fanout_dir(family) else {
            continue;
        };
        let items = link::survey(
            &a.env.in_home(dir),
            a.commons.root(),
            std::slice::from_ref(&entry),
        );
        for item in items {
            // Only the adopted entry is adopt's business — leftovers under
            // other names in the same directory are sync's.
            if item.canonical.is_none() || link::normalize(&item.path) == a.origin {
                continue;
            }
            match item.state {
                link::State::Linked => {}
                state if state.needs_change() => {
                    if dry_run {
                        r.line(format!("  would link into {} ({dir})", target.name));
                    } else if let Err(e) = link::apply(&item) {
                        r.problem(format!("cannot link into {} ({dir}): {e}", target.name));
                    } else {
                        r.line(format!("  linked into {} ({dir})", target.name));
                    }
                }
                // A Variant or a Foreign link is somebody's decision: kept,
                // and named in sync's own words.
                state => r.line(format!(
                    "  {} ({dir}): {} — {}",
                    target.name,
                    state.label(),
                    state.note()
                )),
            }
        }
    }
}

/// The instructions file, fanned out by each agent's own mechanism — exactly
/// what a sync would do for this family, no more.
fn fan_out_instructions(a: &Adoption, r: &mut Reporter, dry_run: bool) {
    // The survey wants an existing Commons file. In a dry run of a first-time
    // absorb it does not exist yet, so the origin stands in: same content, and
    // consulted for nothing beyond existing.
    let commons_file = if a.destination.is_file() {
        a.destination
    } else {
        a.origin
    };

    for item in instructions::survey(a.env, a.config, commons_file) {
        if link::normalize(&item.path) == a.origin {
            continue; // the origin already holds its link
        }
        let place = item
            .path
            .strip_prefix(a.env.home())
            .unwrap_or(&item.path)
            .display()
            .to_string();
        match item.state {
            instructions::State::Linked | instructions::State::ImportPresent => {}
            instructions::State::Missing => {
                if dry_run {
                    r.line(format!("  would link into {} ({place})", item.target));
                } else if let Err(e) = instructions::apply(&item) {
                    r.problem(format!("cannot link into {} ({place}): {e}", item.target));
                } else {
                    r.line(format!("  linked into {} ({place})", item.target));
                }
            }
            instructions::State::ImportMissing => {
                if dry_run {
                    r.line(format!(
                        "  would add the import line to {} ({place})",
                        item.target
                    ));
                } else if let Err(e) = instructions::apply(&item) {
                    r.problem(format!("cannot write {}: {e}", item.path.display()));
                } else {
                    r.line(format!(
                        "  added the import line to {} ({place})",
                        item.target
                    ));
                }
            }
            // Somebody else's file or link: kept, named in sync's own words.
            instructions::State::Conflict | instructions::State::Foreign => {
                r.line(format!("  {}: {}", item.target, item.note()));
            }
        }
    }
}

/// Create the canonical relative link from `path` to `destination`.
fn place_link(path: &Path, destination: &Path) -> std::io::Result<PathBuf> {
    let text = canonical_text(path, destination);
    link::create_symlink(&text, path)?;
    Ok(text)
}

/// The canonical relative link text from `path`'s parent to `destination`.
fn canonical_text(path: &Path, destination: &Path) -> PathBuf {
    link::relative_from(path.parent().unwrap_or(Path::new(".")), destination)
}

/// Where in the Commons an adopted path belongs: which Target it came from,
/// which surface matched, and the Commons-relative path it will occupy.
struct Placement {
    target: String,
    surface: Surface,
    /// Commons-relative destination, e.g. `skills/research` or `AGENTS.md`.
    commons_rel: PathBuf,
}

/// The Target surface an adopted path was found in.
#[derive(Clone, Copy)]
enum Surface {
    /// A family fan-out directory — the entry fans back out to every agent.
    FanOut(Family),
    /// A per-agent instructions destination.
    Instructions,
}

impl Placement {
    /// What to call the family in reports.
    fn family_label(&self) -> &'static str {
        match self.surface {
            Surface::FanOut(family) => family.name(),
            Surface::Instructions => "instructions",
        }
    }
}

/// Match a path against every Target surface agent-sync manages: the fan-out
/// directories, and the per-agent instructions destinations.
fn locate(env: &Env, config: &Config, path: &Path) -> Option<Placement> {
    let parent = link::normalize(path.parent()?);
    let file_name = path.file_name()?.to_string_lossy().into_owned();

    for target in target::resolve(env, config) {
        for family in Family::ALL {
            let Some(dir) = target.fanout_dir(*family) else {
                continue;
            };
            if link::normalize(&env.in_home(dir)) == parent {
                return Some(Placement {
                    target: target.name.clone(),
                    surface: Surface::FanOut(*family),
                    commons_rel: PathBuf::from(family.name()).join(&file_name),
                });
            }
        }

        // Instructions are a single file rather than a directory of entries, so
        // they are matched by full path, not by parent directory.
        if let Some(agent) = target.agent {
            let destination = match agent.instructions {
                Instructions::Symlink(rel) => Some(env.in_home(rel)),
                Instructions::RulesDirLink(dir) => Some(env.in_home(dir).join("AGENTS.md")),
                // Claude's file is the user's own; it is never adopted wholesale.
                Instructions::ImportLine(_) | Instructions::None => None,
            };
            if destination.map(|d| link::normalize(&d)) == Some(link::normalize(path)) {
                return Some(Placement {
                    target: target.name.clone(),
                    surface: Surface::Instructions,
                    commons_rel: PathBuf::from(commons::INSTRUCTIONS),
                });
            }
        }
    }
    None
}

fn absolute(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        link::normalize(&path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        link::normalize(&cwd.join(path))
    }
}

/// Rename where possible, copy across filesystems where not.
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_tree(from, to)?;
            if from.is_dir() {
                fs::remove_dir_all(from)
            } else {
                fs::remove_file(from)
            }
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}
