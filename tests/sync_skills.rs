//! `sync` for the skills family — the link engine every fan-out family rides.

mod common;

use common::Fixture;

/// A machine with every skills fan-out agent plus the native agents —
/// including codex and cursor, flipped Native once their docs promised
/// unconditional ~/.agents/skills reads (2026-08-16).
fn machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".codex");
    f.agent(".pi");
    f.agent(".cursor");
    f.agent(".openclaw");
    f.agent(".hermes");
    f.agent(".config/opencode");
    f
}

#[test]
fn links_every_commons_skill_into_every_fan_out_target() {
    let f = machine();
    f.commons_skill("research");
    f.commons_skill("tdd");

    f.run(&["sync"]).assert_clean();

    for target in [
        ".claude/skills",
        ".pi/agent/skills",
        ".openclaw/skills",
        ".hermes/skills",
    ] {
        for skill in ["research", "tdd"] {
            let rel = format!("{target}/{skill}");
            assert!(f.is_symlink(&rel), "{rel} should be a symlink");
            assert_eq!(
                f.resolves_to(&rel),
                std::fs::canonicalize(f.commons().join("skills").join(skill)).unwrap(),
                "{rel} should resolve into the Commons"
            );
        }
    }
}

#[test]
fn native_agents_receive_no_links() {
    let f = machine();
    f.agent(".gemini");
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    // OpenCode scans the Commons itself; a fan-out would only duplicate names.
    assert!(
        !f.exists(".config/opencode/skills"),
        "native agents must not get a skills directory"
    );
    // Codex and cursor read ~/.agents/skills themselves now — and still read
    // their old fan-out dirs, so a link there would be seen twice.
    for legacy in [".codex/skills", ".cursor/skills"] {
        assert!(
            !f.exists(legacy),
            "{legacy} must not be created for a flipped Native agent"
        );
    }
    // Gemini reads the Commons natively too, and never had a fan-out dir.
    assert!(!f.exists(".gemini/skills"));
}

#[test]
fn agents_without_a_skill_surface_receive_nothing() {
    let f = Fixture::new();
    f.agent(".roo");
    f.agent(".cline");
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert!(!f.exists(".roo/skills"));
    assert!(!f.exists(".cline/skills"));
}

#[test]
fn sync_prunes_our_leftover_links_from_a_flipped_agents_legacy_dir() {
    let f = machine();
    f.commons_skill("research");
    // What the pre-flip registry fanned out: canonical links of ours, which
    // codex and cursor still read beside the Commons — live duplicates.
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    f.symlink(".cursor/skills/research", "../../.agents/skills/research");

    let out = f.run(&["sync"]);

    out.assert_clean()
        .assert_stdout_has("codex (.codex/skills)")
        .assert_stdout_has("cursor (.cursor/skills)")
        .assert_stdout_has("duplicate research");
    assert!(
        !f.present(".codex/skills/research"),
        "our leftover link is a live duplicate — pruned"
    );
    assert!(!f.present(".cursor/skills/research"));
    // The directory itself is the agent's, and stays.
    assert!(f.is_real_dir(".codex/skills"));
}

#[test]
fn the_legacy_prune_leaves_foreign_links_and_real_content_alone() {
    let f = machine();
    f.commons_skill("research");
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    // A link somebody else made, and the agent's own real skill.
    let elsewhere = f.root().join("their-skills").join("theirs");
    std::fs::create_dir_all(&elsewhere).unwrap();
    f.symlink(".codex/skills/theirs", &elsewhere.display().to_string());
    f.file(".codex/skills/handmade/SKILL.md", "codex's own\n");

    f.run(&["sync"]).assert_clean();

    assert!(!f.present(".codex/skills/research"), "ours goes");
    assert_eq!(
        f.link_text(".codex/skills/theirs"),
        common::slashed(&elsewhere),
        "a Foreign link is never touched"
    );
    assert_eq!(
        f.contents(".codex/skills/handmade/SKILL.md"),
        "codex's own\n",
        "real content in the legacy dir is the agent's own"
    );
}

#[test]
fn a_pruned_legacy_dir_converges_and_a_second_sync_is_silent() {
    let f = machine();
    f.commons_skill("research");
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    f.run(&["sync"]).assert_clean();
    let after = f.tree();

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");
    assert_eq!(after, f.tree(), "the prune must converge");
}

#[test]
fn dry_run_reports_the_legacy_prune_without_removing_anything() {
    let f = machine();
    f.commons_skill("research");
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    // Fan-out work is already done, so the prune is the whole plan.
    f.run(&["sync"]).assert_clean();
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    let before = f.tree();

    let out = f.run(&["sync", "--dry-run"]);

    out.assert_clean().assert_stdout_has("duplicate research");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");
}

#[test]
fn links_are_relative_so_the_home_directory_can_move() {
    let f = machine();
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.link_text(".claude/skills/research"),
        "../../.agents/skills/research"
    );
    // Depth is computed, not hand-encoded: pi sits one level deeper.
    assert_eq!(
        f.link_text(".pi/agent/skills/research"),
        "../../../.agents/skills/research"
    );
}

#[test]
fn rewrites_our_own_link_when_it_is_not_in_canonical_form() {
    let f = machine();
    f.commons_skill("research");
    let absolute = f.commons().join("skills").join("research");
    f.symlink(".claude/skills/research", &absolute.display().to_string());

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.link_text(".claude/skills/research"),
        "../../.agents/skills/research",
        "a link resolving into the Commons is ours to canonicalise"
    );
}

#[test]
fn leaves_a_link_that_resolves_outside_the_commons() {
    let f = machine();
    f.commons_skill("research");
    let elsewhere = f.root().join("someone-elses-skills").join("research");
    std::fs::create_dir_all(&elsewhere).unwrap();
    f.symlink(".claude/skills/other", &elsewhere.display().to_string());

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.link_text(".claude/skills/other"),
        common::slashed(&elsewhere),
        "Foreign links are never touched"
    );
}

#[test]
fn prunes_a_dangling_link_that_pointed_into_the_commons() {
    let f = machine();
    f.commons_skill("research");
    // What a skill deleted from the Commons leaves behind.
    f.symlink(".claude/skills/deleted", "../../.agents/skills/deleted");

    f.run(&["sync"]).assert_clean();

    assert!(
        !f.present(".claude/skills/deleted"),
        "a dangling Commons-pointing link should be pruned"
    );
    assert!(f.is_symlink(".claude/skills/research"));
}

#[test]
fn leaves_a_dangling_link_that_pointed_somewhere_else() {
    let f = machine();
    f.symlink(".claude/skills/orphan", "../../../elsewhere/orphan");

    f.run(&["sync"]).assert_clean();

    assert!(
        f.present(".claude/skills/orphan"),
        "Foreign garbage is still Foreign — never ours to delete"
    );
}

#[test]
fn preserves_a_variant_directory() {
    let f = machine();
    f.commons_skill("plannotator");
    f.file(
        ".claude/skills/plannotator/SKILL.md",
        "claude-specific variant\n",
    );

    f.run(&["sync"]).assert_clean();

    assert!(
        f.is_real_dir(".claude/skills/plannotator"),
        "a Variant must survive sync"
    );
    assert_eq!(
        std::fs::read_to_string(f.path(".claude/skills/plannotator/SKILL.md")).unwrap(),
        "claude-specific variant\n"
    );
    // Other agents still get the Commons copy.
    assert!(f.is_symlink(".pi/agent/skills/plannotator"));
}

#[test]
fn reports_what_it_did() {
    let f = machine();
    f.commons_skill("research");

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("research");
}

#[test]
fn a_second_sync_changes_nothing_and_says_so() {
    let f = machine();
    f.commons_skill("research");
    f.commons_skill("tdd");
    f.run(&["sync"]).assert_clean();
    let after_first = f.tree();

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("up to date");
    assert_eq!(after_first, f.tree(), "a second sync must be a no-op");
}

#[test]
fn dry_run_reports_the_plan_and_changes_nothing() {
    let f = machine();
    f.commons_skill("research");
    let before = f.tree();

    let out = f.run(&["sync", "--dry-run"]);

    out.assert_clean().assert_stdout_has("research");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");
}

#[test]
fn creates_missing_target_directories_but_never_agent_roots() {
    let f = Fixture::new();
    f.agent(".claude"); // root exists, skills/ does not
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert!(
        f.is_real_dir(".claude/skills"),
        "subdirectories are created"
    );
    for absent in [".codex", ".pi", ".cursor", ".gemini"] {
        assert!(
            !f.exists(absent),
            "{absent} is not installed and must not be created"
        );
    }
}

#[test]
fn missing_commons_is_an_error_that_names_the_fix() {
    let f = Fixture::bare();
    f.agent(".claude");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("agent-sync init");
}

#[test]
fn a_second_process_holding_the_lock_fails_cleanly() {
    let f = machine();
    f.commons_skill("research");
    let _held = common::hold_lock(f.path(".local/state/agent-sync/lock"));

    let out = f.run_with_vars(
        &["sync"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", f.commons().display().to_string()),
            ("AGENT_SYNC_LOCK_TIMEOUT_MS", "150".to_string()),
        ],
    );

    out.assert_code(1).assert_stderr_has("another agent-sync");
    assert!(
        !f.exists(".claude/skills/research"),
        "the blocked run must not have written anything"
    );
}
