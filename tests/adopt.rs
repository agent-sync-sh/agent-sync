//! `adopt` — the on-ramp from a hand-rolled setup, and the cure for an
//! accidental Variant. One verb, three mechanics, no force flag.

mod common;

use std::collections::BTreeSet;

use common::{Fixture, Outcome};

fn machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".codex");
    // pi is the second skills fan-out target: codex reads the Commons
    // natively now, so cross-agent fan-out assertions land on pi.
    f.agent(".pi");
    f
}

#[test]
fn a_skill_the_commons_does_not_have_moves_in_and_leaves_a_link() {
    let f = machine();
    f.file(".claude/skills/local/SKILL.md", "a hand-written skill\n");

    let out = f.run(&[
        "adopt",
        &f.path(".claude/skills/local").display().to_string(),
    ]);

    out.assert_clean().assert_stdout_has("local");
    assert_eq!(
        std::fs::read_to_string(f.commons().join("skills/local/SKILL.md")).unwrap(),
        "a hand-written skill\n",
        "the content moves into the Commons intact"
    );
    assert_eq!(
        f.link_text(".claude/skills/local"),
        "../../.agents/skills/local",
        "a canonical link is left behind"
    );
}

#[test]
fn an_adopted_skill_reaches_every_other_agent_at_once() {
    let f = machine();
    f.file(".claude/skills/local/SKILL.md", "mine\n");

    let out = f.run(&[
        "adopt",
        &f.path(".claude/skills/local").display().to_string(),
    ]);

    out.assert_clean()
        .assert_stdout_has("linked into pi (.pi/agent/skills)");
    assert_eq!(
        f.link_text(".pi/agent/skills/local"),
        "../../../.agents/skills/local",
        "the fan-out must leave sync's canonical link form"
    );
}

#[test]
fn sync_right_after_an_adopt_has_nothing_to_do() {
    let f = machine();
    f.file(".claude/skills/local/SKILL.md", "mine\n");
    f.run(&[
        "adopt",
        &f.path(".claude/skills/local").display().to_string(),
    ])
    .assert_clean();

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("Everything is up to date.");
}

#[test]
fn the_fan_out_keeps_variants_and_skips_native_agents() {
    let f = machine();
    f.agent(".config/opencode"); // reads the Commons natively — no skills fan-out
    f.agent(".cursor"); // flipped Native 2026-08-16 — likewise
    f.agent(".gemini"); // Native, and never had a fan-out dir
    f.file(".claude/skills/dup/SKILL.md", "the adopted copy\n");
    f.file(".pi/agent/skills/dup/SKILL.md", "the pi variant\n");

    let out = f.run(&["adopt", &f.path(".claude/skills/dup").display().to_string()]);

    out.assert_clean()
        .assert_stdout_has("pi (.pi/agent/skills): variant — left alone");
    assert!(f.is_real_dir(".pi/agent/skills/dup"), "a Variant is kept");
    assert_eq!(
        f.contents(".pi/agent/skills/dup/SKILL.md"),
        "the pi variant\n"
    );
    for native in [
        ".config/opencode/skills/dup",
        ".codex/skills/dup",
        ".cursor/skills/dup",
        ".gemini/skills/dup",
    ] {
        assert!(
            !f.present(native),
            "a Native agent receives nothing: {native}"
        );
    }
}

#[test]
fn an_identical_copy_is_collapsed_into_a_link() {
    let f = machine();
    f.commons_skill("research");
    let body = std::fs::read_to_string(f.commons().join("skills/research/SKILL.md")).unwrap();
    f.file(".claude/skills/research/SKILL.md", &body);

    let out = f.run(&[
        "adopt",
        &f.path(".claude/skills/research").display().to_string(),
    ]);

    out.assert_clean().assert_stdout_has("identical");
    assert!(f.is_symlink(".claude/skills/research"));
    assert_eq!(
        std::fs::read_to_string(f.commons().join("skills/research/SKILL.md")).unwrap(),
        body,
        "the Commons copy is untouched"
    );
}

#[test]
fn a_diverged_copy_is_refused_and_nothing_moves() {
    let f = machine();
    f.commons_skill("plannotator");
    f.file(
        ".claude/skills/plannotator/SKILL.md",
        "the claude variant\n",
    );
    let before = f.tree();

    let out = f.run(&[
        "adopt",
        &f.path(".claude/skills/plannotator").display().to_string(),
    ]);

    out.assert_code(1).assert_stderr_has("Variant");
    assert_eq!(before, f.tree(), "a refusal must change nothing");
}

#[test]
fn a_command_is_adopted_into_the_commands_family() {
    let f = machine();
    f.file(".claude/commands/ship.md", "ship it\n");

    let out = f.run(&[
        "adopt",
        &f.path(".claude/commands/ship.md").display().to_string(),
    ]);

    out.assert_clean().assert_stdout_has("commands");
    assert!(f.commons().join("commands/ship.md").exists());
    assert!(f.is_symlink(".claude/commands/ship.md"));
}

#[test]
fn something_already_linked_has_nothing_to_adopt() {
    let f = machine();
    f.commons_skill("research");
    f.run(&["sync"]).assert_clean();

    f.run(&[
        "adopt",
        &f.path(".claude/skills/research").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has("already a symlink");
}

#[test]
fn a_path_outside_every_target_and_family_is_refused() {
    let f = machine();
    f.file("somewhere/else/thing.md", "not in a target\n");

    f.run(&[
        "adopt",
        &f.path("somewhere/else/thing.md").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has("parent directory \"else\" is not a family")
    .assert_stderr_has("skills/, commands/, or agents/");
}

#[test]
fn a_path_that_does_not_exist_is_refused() {
    let f = machine();

    f.run(&[
        "adopt",
        &f.path(".claude/skills/ghost").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has("does not exist");
}

#[test]
fn an_instructions_file_can_be_adopted_too() {
    let f = machine();
    f.file(".codex/AGENTS.md", "# my shared instructions\n");

    let out = f.run(&["adopt", &f.path(".codex/AGENTS.md").display().to_string()]);

    out.assert_clean()
        .assert_stdout_has("instructions")
        .assert_stdout_has("added the import line to claude (.claude/CLAUDE.md)");
    assert_eq!(
        std::fs::read_to_string(f.commons().join("AGENTS.md")).unwrap(),
        "# my shared instructions\n"
    );
    assert!(f.is_symlink(".codex/AGENTS.md"));

    // The fan-out already happened, exactly as instructions sync applies it.
    assert!(
        f.contents(".claude/CLAUDE.md")
            .contains("@~/.agents/AGENTS.md"),
        "the import line lands during adopt, not on the next sync"
    );
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("Everything is up to date.");
}

#[test]
fn claudes_own_file_is_never_adopted_wholesale() {
    let f = machine();
    f.file(".claude/CLAUDE.md", "# claude's own notes\n");

    // No surface claims it, and `.claude` is no family — the file stays the
    // user's own.
    f.run(&["adopt", &f.path(".claude/CLAUDE.md").display().to_string()])
        .assert_code(1)
        .assert_stderr_has("parent directory \".claude\" is not a family");
}

// ---- the Source mechanic ----

#[test]
fn a_repo_path_becomes_an_absolute_commons_link_and_fans_out() {
    let f = machine();
    f.file("github/skillrepo/.git/HEAD", "ref: refs/heads/main\n");
    f.file(
        "github/skillrepo/skills/research/SKILL.md",
        "kept in the repo\n",
    );
    let path = f.path("github/skillrepo/skills/research");

    let out = f.run(&["adopt", &path.display().to_string()]);

    out.assert_clean()
        .assert_stdout_has(&format!(
            "adopted research — Sourced from {} (skills)",
            path.display()
        ))
        .assert_stdout_has("linked into claude (.claude/skills)")
        .assert_stdout_has("linked into pi (.pi/agent/skills)");
    assert_eq!(
        f.link_text(".agents/skills/research"),
        common::slashed(&path),
        "the Commons link is absolute, the path exactly as given"
    );
    assert_eq!(
        f.link_text(".claude/skills/research"),
        "../../.agents/skills/research",
        "agents link to the Commons entry, which holds the pointer"
    );
    assert_eq!(
        f.contents(".claude/skills/research/SKILL.md"),
        "kept in the repo\n",
        "the content reaches agents through the chain of links"
    );
}

#[test]
fn sync_right_after_a_source_adopt_has_nothing_to_do() {
    let f = machine();
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", "mine\n");
    f.run(&[
        "adopt",
        &f.path("repo/skills/research").display().to_string(),
    ])
    .assert_clean();

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("Everything is up to date.");
}

#[test]
fn a_git_file_marks_a_durable_home_too() {
    let f = machine();
    // A worktree or submodule carries `.git` as a *file* pointing home.
    f.file(
        "worktrees/wt/.git",
        "gitdir: /somewhere/main/.git/worktrees/wt\n",
    );
    f.file("worktrees/wt/commands/ship.md", "ship from the worktree\n");
    let path = f.path("worktrees/wt/commands/ship.md");

    f.run(&["adopt", &path.display().to_string()])
        .assert_clean()
        .assert_stdout_has("Sourced from")
        .assert_stdout_has("(commands)");
    assert_eq!(
        f.link_text(".agents/commands/ship.md"),
        common::slashed(&path)
    );
}

#[test]
fn re_running_a_source_adopt_is_a_clean_no_op() {
    let f = machine();
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", "mine\n");
    let path = f.path("repo/skills/research").display().to_string();
    f.run(&["adopt", &path]).assert_clean();
    let before = f.tree();

    let out = f.run(&["adopt", &path]);

    out.assert_clean().assert_stdout_has(&format!(
        "skills/research is already Sourced from {path} — nothing to do"
    ));
    assert_eq!(before, f.tree(), "a no-op must change nothing");
}

#[test]
fn a_symlink_input_is_linked_as_given_never_resolved_through() {
    let f = machine();
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/lib/research/SKILL.md", "the real copy\n");
    f.symlink("repo/skills/research", "../lib/research");
    let path = f.path("repo/skills/research");

    f.run(&["adopt", &path.display().to_string()])
        .assert_clean();

    assert_eq!(
        f.link_text(".agents/skills/research"),
        common::slashed(&path),
        "the Commons links to the symlink, not through it"
    );
    assert_eq!(
        f.contents(".agents/skills/research/SKILL.md"),
        "the real copy\n",
        "the content still reads through the chain"
    );
}

#[test]
fn an_identical_commons_copy_is_replaced_by_the_link() {
    let f = machine();
    f.commons_skill("research");
    let body = f.contents_of_commons("skills/research/SKILL.md");
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", &body);
    let path = f.path("repo/skills/research");

    let out = f.run(&["adopt", &path.display().to_string()]);

    out.assert_clean()
        .assert_stdout_has("replaced the identical Commons copy with the link");
    assert!(
        f.is_symlink(".agents/skills/research"),
        "the Source holds the single real copy now"
    );
    assert_eq!(
        f.contents(".agents/skills/research/SKILL.md"),
        body,
        "the content still reads through the link"
    );
}

#[test]
fn a_diverged_commons_copy_refuses_the_source() {
    let f = machine();
    f.commons_skill("research");
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", "a different body\n");
    let before = f.tree();

    f.run(&[
        "adopt",
        &f.path("repo/skills/research").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has("Variant")
    .assert_stderr_has("Merge it by hand");
    assert_eq!(before, f.tree(), "a refusal must change nothing");
}

#[test]
fn a_commons_link_pointing_elsewhere_refuses_to_repoint() {
    let f = machine();
    f.file(
        "elsewhere/skills/research/SKILL.md",
        "the other repo's copy\n",
    );
    let other = f.path("elsewhere/skills/research");
    f.commons_symlink("skills/research", &other.display().to_string());
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", "mine\n");

    f.run(&[
        "adopt",
        &f.path("repo/skills/research").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has(&format!(
        "skills/research is already Sourced from {} — \
         remove the Commons link first if you mean to repoint it",
        other.display()
    ));
    assert_eq!(
        f.link_text(".agents/skills/research"),
        common::slashed(&other),
        "the existing link is untouched"
    );
}

#[test]
fn a_repo_path_under_a_non_family_parent_is_refused() {
    let f = machine();
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/prompts/thing.md", "no family claims this\n");

    f.run(&[
        "adopt",
        &f.path("repo/prompts/thing.md").display().to_string(),
    ])
    .assert_code(1)
    .assert_stderr_has("parent directory \"prompts\" is not a family")
    .assert_stderr_has("skills/, commands/, or agents/");
}

// ---- the Copy mechanic ----

#[test]
fn a_family_path_with_no_repo_above_copies_in_and_fans_out() {
    let f = machine();
    f.file("loose/skills/research/SKILL.md", "no durable home\n");
    let path = f.path("loose/skills/research");

    let out = f.run(&["adopt", &path.display().to_string()]);

    out.assert_clean()
        .assert_stdout_has("adopted research — copied into the Commons (skills)")
        .assert_stdout_has(&format!(
            "  the original at {} is no longer consulted; edits there will \
             diverge from the Commons copy",
            path.display()
        ))
        .assert_stdout_has("linked into claude (.claude/skills)")
        .assert_stdout_has("linked into pi (.pi/agent/skills)")
        .assert_stdout_lacks("Sourced");
    assert!(
        !f.is_symlink(".agents/skills/research"),
        "the Commons entry is real — a copy, not a pointer"
    );
    assert_eq!(
        f.contents_of_commons("skills/research/SKILL.md"),
        "no durable home\n"
    );

    // The original is untouched, and nothing anywhere points back at it.
    assert!(f.is_real_dir("loose/skills/research"));
    assert_eq!(
        f.contents("loose/skills/research/SKILL.md"),
        "no durable home\n"
    );
    for entry in f.tree() {
        if let Some((_, target)) = entry.split_once(" -> ") {
            assert!(
                !target.contains("loose"),
                "nothing may point back at the original: {entry}"
            );
        }
    }

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("Everything is up to date.");
}

#[test]
fn a_symlink_input_is_copied_through_to_its_content() {
    let f = machine();
    f.file("lib/research/SKILL.md", "the real copy\n");
    f.symlink("loose/skills/research", "../../lib/research");

    f.run(&[
        "adopt",
        &f.path("loose/skills/research").display().to_string(),
    ])
    .assert_clean();

    assert!(
        !f.is_symlink(".agents/skills/research"),
        "the content is copied through the link — the Commons entry is real"
    );
    assert_eq!(
        f.contents_of_commons("skills/research/SKILL.md"),
        "the real copy\n"
    );
    assert!(
        f.is_symlink("loose/skills/research"),
        "the original link is untouched"
    );
}

#[test]
fn dry_run_of_a_copy_changes_nothing_and_previews_the_real_run() {
    let f = machine();
    f.file("loose/skills/research/SKILL.md", "no durable home\n");
    let path = f.path("loose/skills/research").display().to_string();
    let before = f.tree();

    let dry = f.run(&["adopt", &path, "--dry-run"]);

    dry.assert_clean()
        .assert_stdout_has("dry run — no changes will be made")
        .assert_stdout_has("copy: research — copied into the Commons (skills)")
        .assert_stdout_has(&format!("would copy {path} into the Commons"))
        .assert_stdout_has(&format!(
            "  the original at {path} is no longer consulted; edits there \
             will diverge from the Commons copy"
        ))
        .assert_stdout_has("would link into claude (.claude/skills)")
        .assert_stdout_has("would link into pi (.pi/agent/skills)");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");

    // Every fan-out the dry run promised, the real run delivers verbatim.
    let real = f.run(&["adopt", &path]);
    real.assert_clean()
        .assert_stdout_has("adopted research — copied into the Commons (skills)");
    for line in dry.stdout.lines() {
        if let Some(rest) = line.strip_prefix("  would link into ") {
            assert!(
                real.stdout.contains(&format!("  linked into {rest}")),
                "the dry run promised a link into {rest:?}\nreal run:\n{}",
                real.stdout
            );
        }
    }
}

#[test]
fn a_file_under_a_skills_parent_refuses_the_shape_mismatch() {
    let f = machine();
    f.file(
        "loose/skills/notes.md",
        "a file where a directory belongs\n",
    );
    let path = f.path("loose/skills/notes.md");
    let before = f.tree();

    f.run(&["adopt", &path.display().to_string()])
        .assert_code(1)
        .assert_stderr_has(&format!(
            "{}: skills entries are directories — this is a file",
            path.display()
        ));
    assert_eq!(before, f.tree(), "a refusal must change nothing");
}

#[test]
fn a_directory_under_a_commands_parent_refuses_the_shape_mismatch() {
    let f = machine();
    f.file(
        "loose/commands/ship/notes.md",
        "a directory where a file belongs\n",
    );
    let path = f.path("loose/commands/ship");
    let before = f.tree();

    f.run(&["adopt", &path.display().to_string()])
        .assert_code(1)
        .assert_stderr_has(&format!(
            "{}: commands entries are single .md files — this is a directory",
            path.display()
        ));
    assert_eq!(before, f.tree(), "a refusal must change nothing");
}

#[test]
fn a_declared_custom_surface_absorbs_even_inside_a_repo() {
    let f = machine();
    // Declaring a custom target is an explicit statement that the dir has
    // fan-out semantics — the surface wins over the repo walk-up (ADR-0006).
    f.file(
        ".config/agent-sync/agent-sync.toml",
        "[custom.myagent]\nroot = \"repo/.myagent\"\nskills = \"repo/.myagent/skills\"\n",
    );
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/.myagent/skills/local/SKILL.md", "mine\n");

    let out = f.run(&[
        "adopt",
        &f.path("repo/.myagent/skills/local").display().to_string(),
    ]);

    out.assert_clean()
        .assert_stdout_has("adopted local from myagent into the Commons (skills)")
        .assert_stdout_lacks("Sourced");
    assert!(
        !f.is_symlink(".agents/skills/local"),
        "absorb moves the content in — the Commons entry is real"
    );
    assert!(
        f.is_symlink("repo/.myagent/skills/local"),
        "the surface keeps a link behind, exactly as any absorb"
    );
}

#[test]
fn dry_run_of_a_source_changes_nothing_and_previews_the_real_run() {
    let f = machine();
    f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
    f.file("repo/skills/research/SKILL.md", "mine\n");
    let path = f.path("repo/skills/research").display().to_string();
    let commons_entry = f.commons().join("skills/research");
    let before = f.tree();

    let dry = f.run(&["adopt", &path, "--dry-run"]);

    dry.assert_clean()
        .assert_stdout_has("dry run — no changes will be made")
        .assert_stdout_has(&format!("source: research — Sourced from {path} (skills)"))
        .assert_stdout_has(&format!("would link {} → {path}", commons_entry.display()))
        .assert_stdout_has("would link into claude (.claude/skills)")
        .assert_stdout_has("would link into pi (.pi/agent/skills)");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");

    // Every fan-out the dry run promised, the real run delivers verbatim.
    let real = f.run(&["adopt", &path]);
    real.assert_clean()
        .assert_stdout_has(&format!("adopted research — Sourced from {path} (skills)"));
    for line in dry.stdout.lines() {
        if let Some(rest) = line.strip_prefix("  would link into ") {
            assert!(
                real.stdout.contains(&format!("  linked into {rest}")),
                "the dry run promised a link into {rest:?}\nreal run:\n{}",
                real.stdout
            );
        }
    }
}

// ---- the Commons guard ----

#[test]
fn an_entry_of_a_committed_commons_is_refused_and_left_intact() {
    let f = machine();
    // The Commons is meant to be committed. A `.git` at its root must never
    // make adopt classify its own entries as repo-backed and self-link them —
    // the guard answers before any repo detection can.
    std::fs::create_dir_all(f.commons().join(".git")).unwrap();
    f.commons_skill("research");
    let before = f.tree();

    let entry = f.commons().join("skills/research");
    let out = f.run(&["adopt", &entry.display().to_string()]);

    out.assert_code(1)
        .assert_stderr_has("already in the Commons — nothing to adopt");
    assert_eq!(before, f.tree(), "the refusal must change nothing");
    assert!(
        std::fs::read_to_string(entry.join("SKILL.md"))
            .unwrap()
            .contains("body of research"),
        "the entry must survive intact"
    );
}

#[test]
fn the_commons_guard_answers_before_the_symlink_refusal() {
    let f = machine();
    f.commons_skill("research");
    // A symlink inside the Commons must hit the guard, not the symlink
    // refusal: the guard is first in dispatch.
    f.commons_symlink("skills/linked", "research");

    let entry = f.commons().join("skills/linked");
    f.run(&["adopt", &entry.display().to_string()])
        .assert_code(1)
        .assert_stderr_has("already in the Commons")
        .assert_stderr_lacks("already a symlink");
}

// ---- --dry-run ----

#[test]
fn dry_run_changes_nothing_and_previews_the_real_run() {
    let f = machine();
    f.file(".claude/skills/local/SKILL.md", "mine\n");
    let path = f.path(".claude/skills/local").display().to_string();
    let before = f.tree();

    let dry = f.run(&["adopt", &path, "--dry-run"]);

    dry.assert_clean()
        .assert_stdout_has("dry run — no changes will be made")
        .assert_stdout_has("absorb: local from claude into the Commons (skills)")
        .assert_stdout_has(&format!("would move {path} into the Commons"))
        .assert_stdout_has(&format!("would leave a link at {path} → "))
        .assert_stdout_has("would link into pi (.pi/agent/skills)");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");

    // Every fan-out the dry run promised, the real run delivers verbatim.
    let real = f.run(&["adopt", &path]);
    real.assert_clean()
        .assert_stdout_has("adopted local from claude into the Commons (skills)");
    for line in dry.stdout.lines() {
        if let Some(rest) = line.strip_prefix("  would link into ") {
            assert!(
                real.stdout.contains(&format!("  linked into {rest}")),
                "the dry run promised a link into {rest:?}\nreal run:\n{}",
                real.stdout
            );
        }
    }
}

#[test]
fn dry_run_previews_the_instructions_fan_out_too() {
    let f = machine();
    f.file(".codex/AGENTS.md", "# my shared instructions\n");
    let before = f.tree();

    f.run(&[
        "adopt",
        &f.path(".codex/AGENTS.md").display().to_string(),
        "--dry-run",
    ])
    .assert_clean()
    .assert_stdout_has("absorb: AGENTS.md from codex into the Commons (instructions)")
    .assert_stdout_has("would add the import line to claude (.claude/CLAUDE.md)");
    assert_eq!(before, f.tree(), "--dry-run must not touch the filesystem");
}

#[test]
fn dry_run_refuses_in_the_same_words_as_a_real_run() {
    let f = machine();
    f.commons_skill("plannotator");
    f.file(
        ".claude/skills/plannotator/SKILL.md",
        "the claude variant\n",
    );
    let path = f.path(".claude/skills/plannotator").display().to_string();
    let before = f.tree();

    let dry = f.run(&["adopt", &path, "--dry-run"]);
    dry.assert_code(1).assert_stderr_has("Variant");
    assert_eq!(before, f.tree(), "a dry-run refusal must change nothing");

    let real = f.run(&["adopt", &path]);
    real.assert_code(1);
    assert_eq!(dry.stderr, real.stderr, "same refusal, same words");
}

#[test]
fn dry_run_never_contends_for_the_lock() {
    let f = machine();
    f.file(".claude/skills/local/SKILL.md", "mine\n");
    let _held = common::hold_lock(f.path(".local/state/agent-sync/lock"));

    f.run_with_vars(
        &[
            "adopt",
            &f.path(".claude/skills/local").display().to_string(),
            "--dry-run",
        ],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", f.commons().display().to_string()),
            ("AGENT_SYNC_LOCK_TIMEOUT_MS", "150".to_string()),
        ],
    )
    .assert_clean();
}

// ---- the collision matrix, swept across every mechanic ----
//
// Four Commons collisions × three mechanics, driven through one harness. The
// per-mechanic *action* differs only where ADR-0006 says it does — an
// identical Commons copy makes absorb relink, source replace, copy no-op —
// and every shared refusal and no-op must use byte-identical wording once the
// fixture-specific paths are masked out.

/// One mechanic's run of one collision case.
struct Sweep {
    mechanic: &'static str,
    /// The adopted path — absolute, fixture-specific.
    path: String,
    home: String,
    outcome: Outcome,
    before: BTreeSet<String>,
    after: BTreeSet<String>,
}

impl Sweep {
    /// The output with fixture-specific paths masked, so wording can be
    /// compared byte-for-byte across mechanics.
    fn masked(&self, text: &str) -> String {
        text.replace(&self.path, "<path>")
            .replace(&self.home, "<home>")
    }

    fn assert_untouched(&self) {
        assert_eq!(
            self.before, self.after,
            "{}: a refusal or no-op must change nothing",
            self.mechanic
        );
    }
}

/// Run one collision case through all three mechanics: the same skill body at
/// a Target-surface path (absorb), a repo path (source), and a loose path
/// (copy), with `prepare` staging the Commons collision before each run.
fn sweep(prepare: impl Fn(&Fixture, &str)) -> Vec<Sweep> {
    ["absorb", "source", "copy"]
        .into_iter()
        .map(|mechanic| {
            let f = machine();
            let rel = match mechanic {
                "absorb" => ".claude/skills/research",
                "source" => "repo/skills/research",
                _ => "loose/skills/research",
            };
            if mechanic == "source" {
                f.file("repo/.git/HEAD", "ref: refs/heads/main\n");
            }
            f.file(&format!("{rel}/SKILL.md"), "the outside copy\n");
            let path = f.path(rel).display().to_string();
            prepare(&f, &path);
            let before = f.tree();
            let outcome = f.run(&["adopt", &path]);
            Sweep {
                mechanic,
                home: f.home().display().to_string(),
                after: f.tree(),
                path,
                outcome,
                before,
            }
        })
        .collect()
}

/// Every masked voice must be byte-identical: one matrix, one wording.
fn assert_one_voice(voices: &[String]) {
    assert!(
        voices.windows(2).all(|w| w[0] == w[1]),
        "the matrix must speak with one voice across mechanics:\n{voices:#?}"
    );
}

#[test]
fn matrix_an_identical_commons_copy_answers_per_mechanic() {
    let runs = sweep(|f, _| {
        f.commons_file("skills/research/SKILL.md", "the outside copy\n");
    });
    let [absorb, source, copy] = &runs[..] else {
        unreachable!()
    };

    absorb
        .outcome
        .assert_clean()
        .assert_stdout_has("identical to the Commons copy — re-linked");
    source
        .outcome
        .assert_clean()
        .assert_stdout_has("replaced the identical Commons copy with the link");
    copy.outcome
        .assert_clean()
        .assert_stdout_has("skills/research is already in the Commons — nothing to do");
    copy.assert_untouched(); // the original stays — no deletion, no link back
}

#[test]
fn matrix_a_divergent_commons_copy_refuses_in_one_voice() {
    let mut voices = Vec::new();
    for s in sweep(|f, _| {
        f.commons_file("skills/research/SKILL.md", "the commons variant\n");
    }) {
        s.outcome
            .assert_code(1)
            .assert_stderr_has("this is a Variant")
            .assert_stderr_has("Merge it by hand");
        s.assert_untouched();
        voices.push(s.masked(&s.outcome.stderr));
    }
    assert_one_voice(&voices);
}

#[test]
fn matrix_a_commons_link_to_the_same_place_is_a_no_op_everywhere() {
    let mut voices = Vec::new();
    for s in sweep(|f, path| {
        f.commons_symlink("skills/research", path);
    }) {
        s.outcome.assert_clean().assert_stdout_has(&format!(
            "skills/research is already Sourced from {} — nothing to do",
            s.path
        ));
        s.assert_untouched();
        voices.push(s.masked(&s.outcome.stdout));
    }
    assert_one_voice(&voices);
}

#[test]
fn matrix_a_commons_link_pointing_elsewhere_refuses_in_one_voice() {
    let mut voices = Vec::new();
    for s in sweep(|f, _| {
        f.file("elsewhere/skills/research/SKILL.md", "the other copy\n");
        f.commons_symlink(
            "skills/research",
            &f.path("elsewhere/skills/research").display().to_string(),
        );
    }) {
        s.outcome
            .assert_code(1)
            .assert_stderr_has("is already Sourced from")
            .assert_stderr_has("remove the Commons link first if you mean to repoint it");
        s.assert_untouched();
        voices.push(s.masked(&s.outcome.stderr));
    }
    assert_one_voice(&voices);
}
