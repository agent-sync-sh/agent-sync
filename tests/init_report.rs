//! `init` on a machine that already has agent configs — the two-minute on-ramp.

mod common;

use common::Fixture;

/// A machine with real, hand-made agent config already on it.
fn populated() -> Fixture {
    let f = Fixture::bare();
    f.agent(".claude");
    f.agent(".codex");
    f.agent(".pi");
    f.agent(".config/opencode");
    // Hand-written skills nobody has adopted yet. pi carries the second one:
    // codex takes no skills fan-out since the 2026-08-16 Native flip, so its
    // dir is no longer an adoption surface init scans.
    f.file(".claude/skills/local-thing/SKILL.md", "mine\n");
    f.file(".pi/agent/skills/another/SKILL.md", "also mine\n");
    // A command, likewise.
    f.file(".claude/commands/ship.md", "ship it\n");
    // MCP servers configured by hand.
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"serena": {"command": "uvx"}, "xapi": {"command": "xurl"}}}"#,
    );
    f
}

#[test]
fn the_report_names_the_agents_it_found() {
    let f = populated();

    let out = f.run(&["init"]);

    out.assert_clean()
        .assert_stdout_has("claude")
        .assert_stdout_has("codex")
        .assert_stdout_has("opencode");
}

#[test]
fn the_report_counts_what_could_be_adopted() {
    let f = populated();

    let out = f.run(&["init"]);

    out.assert_clean()
        .assert_stdout_has("local-thing")
        .assert_stdout_has("another")
        .assert_stdout_has("agent-sync adopt");
}

#[test]
fn the_report_offers_the_mcp_adoption_command() {
    let f = populated();

    let out = f.run(&["init"]);

    out.assert_clean()
        .assert_stdout_has("serena")
        .assert_stdout_has("agent-sync mcp adopt");
}

#[test]
fn the_report_names_a_conflict_and_its_remedy() {
    let f = populated();
    f.commons_file("AGENTS.md", "# shared\n");
    // Another tool already owns opencode's instructions file.
    f.file(".config/opencode/AGENTS.md", "<claude-mem-context>\n");

    let out = f.run(&["init"]);

    out.assert_clean()
        .assert_stdout_has("conflict")
        .assert_stdout_has("move or merge");
}

#[test]
fn a_machine_with_nothing_to_adopt_says_so() {
    let f = Fixture::bare();
    f.agent(".claude");

    let out = f.run(&["init"]);

    out.assert_clean().assert_stdout_has("Nothing to adopt");
}

#[test]
fn the_report_uses_the_projects_vocabulary() {
    let f = populated();

    let out = f.run(&["init"]);

    out.assert_clean().assert_stdout_has("Commons");
}

#[test]
fn init_still_creates_only_the_commons() {
    let f = populated();

    f.run(&["init"]).assert_clean();

    assert!(f.commons().join("skills").is_dir());
    for agent in agent_sync::registry::AGENTS {
        let root = f.path(agent.root);
        if !["claude", "codex", "pi", "opencode"].contains(&agent.name) {
            assert!(!root.exists(), "init created a root for {}", agent.name);
        }
    }
    // Nothing was adopted, only reported.
    assert!(f.is_real_dir(".claude/skills/local-thing"));
    assert!(!f.commons().join("skills/local-thing").exists());
}

#[test]
fn running_init_again_reprints_the_report() {
    let f = populated();
    f.run(&["init"]).assert_clean();

    let out = f.run(&["init"]);

    out.assert_clean()
        .assert_stdout_has("already present")
        .assert_stdout_has("local-thing");
}

#[test]
fn the_report_reflects_work_already_done() {
    let f = populated();
    f.run(&["init"]).assert_clean();
    f.run(&[
        "adopt",
        &f.path(".claude/skills/local-thing").display().to_string(),
    ])
    .assert_clean();

    let out = f.run(&["init"]);

    out.assert_clean().assert_stdout_lacks("local-thing");
}

#[test]
fn the_report_never_prints_a_secret_it_found() {
    let f = populated();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "env": {"TOKEN": "sk-INIT-CANARY"}}}}"#,
    );

    f.run(&["init"])
        .assert_clean()
        .assert_no_output_contains("sk-INIT-CANARY");
}
