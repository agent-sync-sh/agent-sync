//! `revert <agent>` — deliberate offboarding. Everything agent-sync put into
//! one target goes; Foreign content and Variants stay byte-for-byte; a target
//! that is still enabled refuses with the exact config line to add.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [".claude", ".gemini", ".codex", ".pi"] {
        f.agent(root);
    }
    f
}

fn config(f: &Fixture, body: &str) {
    f.file(".config/agent-sync/agent-sync.toml", body);
}

/// Every hook object anywhere under one event, flattened.
fn hooks_at(f: &Fixture, rel: &str, event: &str) -> Vec<serde_json::Value> {
    let config = f.json(rel);
    config["hooks"][event]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .filter_map(|g| g["hooks"].as_array())
                .flatten()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

// ------------------------------------------------------------------ refusals

#[test]
fn revert_on_an_enabled_agent_refuses_with_the_exact_disable_line() {
    let f = machine();
    f.commons_skill("research");
    f.run(&["sync"]).assert_clean();
    let before = f.tree();

    let out = f.run(&["revert", "claude"]);
    out.assert_code(1).assert_stderr_has(&format!(
        "claude is still enabled — set targets.claude = false in {} first, \
         so sync will not redo what revert undoes",
        f.path(".config/agent-sync/agent-sync.toml").display()
    ));
    assert_eq!(before, f.tree(), "a refusal must change nothing");
}

#[test]
fn an_unknown_agent_refuses_by_name() {
    let f = machine();

    f.run(&["revert", "nosuchagent"])
        .assert_code(1)
        .assert_stderr_has("`nosuchagent` is not an agent agent-sync knows about");
}

#[test]
fn a_disabled_agent_that_is_not_installed_is_nothing_to_do() {
    let f = Fixture::new();
    config(&f, "[targets]\nwindsurf = false\n");

    f.run(&["revert", "windsurf"])
        .assert_clean()
        .assert_stdout_has("windsurf is not installed on this machine — nothing to revert");
}

#[test]
fn a_refusal_never_contends_for_the_lock() {
    let f = machine();
    f.commons_skill("research");
    f.run(&["sync"]).assert_clean();
    let _held = common::hold_lock(f.path(".local/state/agent-sync/lock"));

    f.run_with_vars(
        &["revert", "claude"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", f.commons().display().to_string()),
            ("AGENT_SYNC_LOCK_TIMEOUT_MS", "150".to_string()),
        ],
    )
    .assert_code(1)
    .assert_stderr_has("still enabled");
}

#[test]
fn the_teardown_itself_runs_under_the_lock() {
    let f = machine();
    config(&f, "[targets]\nclaude = false\n");
    let _held = common::hold_lock(f.path(".local/state/agent-sync/lock"));

    f.run_with_vars(
        &["revert", "claude"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", f.commons().display().to_string()),
            ("AGENT_SYNC_LOCK_TIMEOUT_MS", "150".to_string()),
        ],
    )
    .assert_code(1)
    .assert_stderr_has("another agent-sync process is running");
}

// ------------------------------------------------------------------ teardown

#[test]
fn revert_strips_every_family_and_touches_nothing_else() {
    let f = machine();
    f.commons_skill("research");
    f.commons_skill("plannotator");
    f.commons_file("AGENTS.md", "shared instructions\n");
    f.commons_mcp(r#"{"mcpServers": {"figma": {"type": "stdio", "command": "figma-mcp"}}}"#);
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"notify session\"\n",
    );

    // Claude's own content, which must survive byte-for-byte: its CLAUDE.md,
    // a Variant of a Commons skill, a Foreign link, a foreign MCP server and
    // a foreign hook sharing our config files.
    f.file(".claude/CLAUDE.md", "# my own notes\n");
    f.file(
        ".claude/skills/plannotator/SKILL.md",
        "the claude variant\n",
    );
    f.symlink(".claude/skills/elsewhere", "../../other/skills/elsewhere");
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"foreign": {"type": "stdio", "command": "theirs"}}, "theme": "dark"}"#,
    );
    f.file(
        ".claude/settings.json",
        r#"{"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "their-hook"}]}]}, "model": "opus"}"#,
    );

    f.run(&["sync"]).assert_clean();
    assert!(f.is_symlink(".claude/skills/research"));

    config(&f, "[targets]\nclaude = false\n");
    let out = f.run(&["revert", "claude"]);
    out.assert_clean()
        .assert_stdout_has("removed .claude/skills/research")
        .assert_stdout_has("removed the import line from .claude/CLAUDE.md")
        .assert_stdout_has("removed mcp server figma from .claude.json")
        .assert_stdout_has(
            "removed the SessionStart hook notify session from .claude/settings.json",
        )
        .assert_stdout_has("4 removals made.");

    // Everything of ours is gone.
    assert!(!f.present(".claude/skills/research"));
    let claude_json = f.json(".claude.json");
    assert!(claude_json["mcpServers"].get("figma").is_none());
    let session = hooks_at(&f, ".claude/settings.json", "SessionStart");
    assert_eq!(session.len(), 1, "only the foreign hook stays: {session:?}");
    assert_eq!(session[0]["command"], "their-hook");

    // Everything of claude's survives, byte-for-byte where bytes are ours to keep.
    assert_eq!(f.contents(".claude/CLAUDE.md"), "# my own notes\n");
    assert_eq!(
        f.contents(".claude/skills/plannotator/SKILL.md"),
        "the claude variant\n"
    );
    assert_eq!(
        f.link_text(".claude/skills/elsewhere"),
        "../../other/skills/elsewhere"
    );
    assert_eq!(claude_json["mcpServers"]["foreign"]["command"], "theirs");
    assert_eq!(claude_json["theme"], "dark");
    assert_eq!(f.json(".claude/settings.json")["model"], "opus");

    // The directories themselves stay: they are detection roots, or claude's own.
    assert!(f.is_real_dir(".claude"));
    assert!(f.is_real_dir(".claude/skills"));

    // Other agents were never in scope.
    assert!(f.is_symlink(".pi/agent/skills/research"));
    assert!(f.is_symlink(".codex/AGENTS.md"));
}

#[test]
fn revert_strips_our_leftovers_from_a_flipped_agents_legacy_dir() {
    // Codex takes no skills fan-out any more, but links an earlier registry
    // put in .codex/skills are still agent-sync's to remove on offboarding.
    let f = machine();
    f.commons_skill("research");
    f.symlink(".codex/skills/research", "../../.agents/skills/research");
    f.file(".codex/skills/handmade/SKILL.md", "codex's own\n");

    config(&f, "[targets]\ncodex = false\n");
    let out = f.run(&["revert", "codex"]);

    out.assert_clean()
        .assert_stdout_has("removed .codex/skills/research");
    assert!(!f.present(".codex/skills/research"));
    assert_eq!(
        f.contents(".codex/skills/handmade/SKILL.md"),
        "codex's own\n",
        "the agent's own content survives"
    );
}

#[test]
fn revert_removes_marked_rendered_files_and_keeps_unmarked_ones() {
    let f = machine();
    f.commons_file(
        "commands/ship.md",
        "---\ndescription: Ship the current branch\n---\n\nDo the thing.\n",
    );
    f.run(&["sync"]).assert_clean();
    assert!(f.exists(".gemini/commands/ship.toml"));
    f.file(
        ".gemini/commands/theirs.toml",
        "description = \"handwritten\"\n",
    );

    config(&f, "[targets]\ngemini = false\n");
    let out = f.run(&["revert", "gemini"]);
    out.assert_clean()
        .assert_stdout_has("removed .gemini/commands/ship.toml");

    assert!(!f.present(".gemini/commands/ship.toml"));
    assert_eq!(
        f.contents(".gemini/commands/theirs.toml"),
        "description = \"handwritten\"\n",
        "a file without the Marker is not ours to remove"
    );
}

#[test]
fn a_disabled_custom_target_is_revertible_and_stays_reverted() {
    let f = Fixture::new();
    f.agent(".myagent");
    f.commons_skill("research");
    config(
        &f,
        "[custom.myagent]\nroot = \".myagent\"\nskills = \".myagent/skills\"\n",
    );

    f.run(&["sync"]).assert_clean();
    assert!(f.is_symlink(".myagent/skills/research"));

    config(
        &f,
        "[targets]\nmyagent = false\n\n[custom.myagent]\nroot = \".myagent\"\nskills = \".myagent/skills\"\n",
    );
    f.run(&["revert", "myagent"])
        .assert_clean()
        .assert_stdout_has("removed .myagent/skills/research");
    assert!(!f.present(".myagent/skills/research"));

    let after = f.tree();
    f.run(&["sync"]).assert_clean();
    assert_eq!(
        after,
        f.tree(),
        "a disabled config-defined target must stay reverted"
    );
}

// ------------------------------------------------------- nothing comes back

#[test]
fn sync_after_revert_recreates_nothing_and_reports_skip_it() {
    let f = machine();
    f.commons_skill("research");
    f.commons_file("AGENTS.md", "shared instructions\n");
    f.commons_mcp(r#"{"mcpServers": {"figma": {"type": "stdio", "command": "figma-mcp"}}}"#);
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"notify session\"\n",
    );
    f.run(&["sync"]).assert_clean();

    config(&f, "[targets]\nclaude = false\n");
    f.run(&["revert", "claude"]).assert_clean();
    let after = f.tree();

    f.run(&["sync"]).assert_clean();
    assert_eq!(after, f.tree(), "sync must not redo what revert undid");

    f.run(&["status"]).assert_stdout_lacks("claude");
    f.run(&["doctor"]).assert_stdout_lacks("claude");
}

#[test]
fn revert_is_idempotent() {
    let f = machine();
    f.commons_skill("research");
    f.run(&["sync"]).assert_clean();
    config(&f, "[targets]\nclaude = false\n");

    f.run(&["revert", "claude"])
        .assert_clean()
        .assert_stdout_has("1 removal made.");
    let after = f.tree();

    f.run(&["revert", "claude"])
        .assert_clean()
        .assert_stdout_has("nothing of agent-sync's in claude — nothing to revert");
    assert_eq!(after, f.tree(), "a second revert must change nothing");
}
