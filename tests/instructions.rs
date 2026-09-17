//! The instructions family — one Commons `AGENTS.md`, three mechanisms, and a
//! conflict where another tool already owns the destination.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [
        ".claude",
        ".codex",
        ".pi",
        ".omp",
        ".roo",
        ".config/opencode",
        ".codeium/windsurf",
        ".cursor",
    ] {
        f.agent(root);
    }
    f.commons_file("AGENTS.md", "# shared instructions\n");
    f
}

#[test]
fn agents_that_can_take_a_symlink_get_one() {
    let f = machine();

    f.run(&["sync"]).assert_clean();

    for rel in [
        ".codex/AGENTS.md",
        ".pi/agent/AGENTS.md",
        ".omp/agent/AGENTS.md",
        ".config/opencode/AGENTS.md",
        ".codeium/windsurf/memories/global_rules.md",
    ] {
        assert!(f.is_symlink(rel), "{rel} should be a symlink");
        assert_eq!(
            std::fs::read_to_string(f.path(rel)).unwrap(),
            "# shared instructions\n"
        );
    }
}

#[test]
fn roo_gets_a_link_inside_its_rules_directory() {
    let f = machine();

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".roo/rules/AGENTS.md"));
}

#[test]
fn claude_gets_an_import_line_not_a_symlink() {
    let f = machine();
    f.file(
        ".claude/CLAUDE.md",
        "# my own notes\n\nsomething claude-specific\n",
    );

    f.run(&["sync"]).assert_clean();

    let body = std::fs::read_to_string(f.path(".claude/CLAUDE.md")).unwrap();
    assert!(
        body.contains("@~/.agents/AGENTS.md"),
        "the import line should be present: {body}"
    );
    assert!(
        body.contains("something claude-specific"),
        "the user's own content must survive: {body}"
    );
    assert!(!f.is_symlink(".claude/CLAUDE.md"));
}

#[test]
fn the_import_line_is_added_only_once() {
    let f = machine();
    f.file(".claude/CLAUDE.md", "# mine\n");
    f.run(&["sync"]).assert_clean();
    let after_first = std::fs::read_to_string(f.path(".claude/CLAUDE.md")).unwrap();

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        std::fs::read_to_string(f.path(".claude/CLAUDE.md")).unwrap(),
        after_first,
        "ensuring the import line must be idempotent"
    );
    assert_eq!(after_first.matches("@~/.agents/AGENTS.md").count(), 1);
}

#[test]
fn a_file_owned_by_another_tool_is_a_conflict_not_an_overwrite() {
    let f = machine();
    // What claude-mem does to opencode's and Gemini's instruction files.
    f.file(".config/opencode/AGENTS.md", "<claude-mem-context>\n");

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("conflict");
    assert_eq!(
        std::fs::read_to_string(f.path(".config/opencode/AGENTS.md")).unwrap(),
        "<claude-mem-context>\n",
        "another tool's file must never be overwritten"
    );
}

#[test]
fn a_conflict_names_the_remedy() {
    let f = machine();
    f.file(".config/opencode/AGENTS.md", "someone else's file\n");

    f.run(&["status"]).assert_stdout_has("move or merge it");
}

#[test]
fn a_conflict_does_not_keep_the_machine_permanently_dirty() {
    let f = machine();
    f.file(".config/opencode/AGENTS.md", "someone else's file\n");
    f.run(&["sync"]).assert_clean();

    // The conflict is reported, but it is a decision for the user about another
    // tool's file — not something `sync` can resolve, so not "actionable".
    f.run(&["status"])
        .assert_code(0)
        .assert_stdout_has("conflict");
}

#[test]
fn agents_without_an_instructions_surface_get_nothing() {
    let f = machine();

    f.run(&["sync"]).assert_clean();

    // Cursor's user-level rules live in app storage, out of agent-sync's reach.
    assert!(!f.present(".cursor/AGENTS.md"));
}

#[test]
fn no_commons_instructions_means_no_instructions_work() {
    let f = Fixture::new();
    f.agent(".codex");
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert!(
        !f.present(".codex/AGENTS.md"),
        "an absent Commons file is a family the user does not use, not a problem"
    );
}

#[test]
fn status_reports_the_instructions_family() {
    let f = machine();

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = parsed["instructions"].as_array().unwrap();
    let codex = entries.iter().find(|e| e["agent"] == "codex").unwrap();
    assert_eq!(codex["state"], "missing");
}

#[test]
fn a_second_sync_leaves_the_instructions_alone() {
    let f = machine();
    f.run(&["sync"]).assert_clean();
    let after_first = f.tree();

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(after_first, f.tree());
}

#[test]
fn prose_mentioning_the_commons_path_is_not_an_import() {
    let f = machine();
    // The path appears, but no line actually imports it.
    f.file(
        ".claude/CLAUDE.md",
        "# notes\n\nInstructions live in ~/.agents/AGENTS.md — edit there.\n",
    );

    f.run(&["sync"]).assert_clean();

    let body = std::fs::read_to_string(f.path(".claude/CLAUDE.md")).unwrap();
    assert!(
        body.lines().any(|l| l.trim() == "@~/.agents/AGENTS.md"),
        "a real import line must still be added: {body}"
    );
}

#[test]
fn a_conflict_names_the_tool_that_owns_the_file() {
    let f = machine();
    f.file(
        ".config/opencode/AGENTS.md",
        "<claude-mem-context>\nstuff\n",
    );

    f.run(&["status"]).assert_stdout_has("owned by claude-mem");
}
