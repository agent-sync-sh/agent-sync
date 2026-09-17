//! Two-phase sync (ADR-0007): every family is surveyed before anything is
//! written, every survey problem is reported in one run, and `--dry-run` is
//! the same plan the real run executes.

mod common;

use std::collections::BTreeSet;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use common::Fixture;

/// A machine where several families all have pending work.
fn machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".codex");
    f.agent(".gemini");
    // A second skills fan-out target: codex reads the Commons natively now.
    f.agent(".pi");
    f
}

/// The home tree minus agent-sync's own state directory: the lock file is the
/// one legitimate write a refused sync still makes.
fn tree_sans_state(f: &Fixture) -> BTreeSet<String> {
    f.tree()
        .into_iter()
        .filter(|entry| !entry.contains(".local"))
        .collect()
}

#[test]
fn a_survey_problem_in_one_family_blocks_every_write_in_every_family() {
    let f = machine();
    f.commons_skill("research");
    f.commons_mcp("{ this is not json");
    let before = tree_sans_state(&f);

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stderr_has("mcp.json")
        .assert_stdout_has("No changes were made");
    assert!(
        !f.present(".claude/skills/research"),
        "the pending symlink must not land while any survey problem exists"
    );
    assert_eq!(before, tree_sans_state(&f), "nothing at all was written");
}

#[test]
fn an_unset_variable_blocks_pending_symlink_work_too() {
    let f = machine();
    f.commons_skill("research");
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"command": "x", "env": {"K": "${env:ABSENT_TOKEN}"}}}}"#,
    );

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stderr_has("ABSENT_TOKEN")
        .assert_stdout_has("No changes were made");
    assert!(!f.present(".claude/skills/research"));
    assert!(!f.present(".claude.json"));
}

#[test]
fn every_unset_variable_is_named_in_one_run() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {
             "first": {"command": "x", "env": {"K": "${env:FIRST_MISSING}"}},
             "second": {"command": "y", "env": {"K": "${env:SECOND_MISSING}"}}}}"#,
    );

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stderr_has("FIRST_MISSING")
        .assert_stderr_has("SECOND_MISSING");
}

#[test]
fn problems_from_different_families_are_reported_together() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"api": {"command": "x", "env": {"K": "${env:GONE}"}}}}"#);
    f.commons_file("hooks/SessionStart.toml", "this is not [ toml");

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stderr_has("GONE")
        .assert_stderr_has("SessionStart.toml");
}

#[test]
fn a_broken_hook_file_and_an_unknown_event_are_both_named() {
    let f = machine();
    f.commons_file("hooks/SessionStart.toml", "this is not [ toml");
    f.commons_file("hooks/NoSuchMoment.toml", "[[hook]]\ncommand = \"x\"\n");

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stderr_has("SessionStart.toml")
        .assert_stderr_has("NoSuchMoment");
}

#[test]
fn dry_run_prints_exactly_the_plan_the_real_run_executes() {
    let f = machine();
    f.commons_skill("research");
    f.commons_file(
        "commands/ship.md",
        "---\ndescription: Ship\n---\n\nShip it.\n",
    );
    f.commons_file("agents/reviewer.md", "review\n");
    f.commons_file("AGENTS.md", "# shared instructions\n");
    f.commons_mcp(r#"{"mcpServers": {"serena": {"command": "uvx"}}}"#);
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"notify\"\n",
    );

    let dry = f.run(&["sync", "--dry-run"]);
    dry.assert_clean();
    let real = f.run(&["sync"]);
    real.assert_clean();

    // Same code path, same lines: only the banner and the summary tense differ.
    let dry_plan = dry
        .stdout
        .strip_prefix("dry run — no changes will be made\n\n")
        .expect("the dry-run banner")
        .replace("would be made.", "made.");
    assert_eq!(
        dry_plan, real.stdout,
        "the dry run must print the plan the real run executes"
    );
}

/// Unix-only: the fault is injected by sealing a directory to 0o555, and
/// Windows has no mode bit that denies a write to its owner.
#[cfg(unix)]
#[test]
fn a_write_failure_reports_continues_and_a_rerun_converges() {
    // Root ignores permission bits, so this proof is meaningless as uid 0.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    let f = machine();
    f.commons_skill("research");
    f.dir(".claude/skills");
    let sealed = std::fs::Permissions::from_mode(0o555);
    std::fs::set_permissions(f.path(".claude/skills"), sealed).unwrap();

    let out = f.run(&["sync"]);

    // The failed write is reported, the remaining operations still ran.
    out.assert_code(1).assert_stderr_has("cannot fix");
    assert!(
        f.is_symlink(".pi/agent/skills/research"),
        "a write failure must not cancel the rest of the plan"
    );
    assert!(!f.present(".claude/skills/research"));

    // Fixing the fault and re-running converges.
    let unsealed = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(f.path(".claude/skills"), unsealed).unwrap();
    f.run(&["sync"]).assert_clean();
    assert!(f.is_symlink(".claude/skills/research"));
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");
}
