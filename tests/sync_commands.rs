//! `sync` for the markdown families — slash commands and agents ride the
//! same link engine as skills, with per-agent destinations from the registry.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [
        ".claude",
        ".codex",
        ".config/opencode",
        ".cursor",
        ".roo",
        ".codeium/windsurf",
        ".gemini",
        ".pi",
    ] {
        f.agent(root);
    }
    f
}

#[test]
fn commands_reach_every_agent_that_reads_markdown_commands() {
    let f = machine();
    f.commons_file("commands/ship.md", "---\nname: ship\n---\n\nship it\n");

    f.run(&["sync"]).assert_clean();

    for dir in [
        ".claude/commands",
        ".codex/prompts",
        ".config/opencode/commands",
        ".cursor/commands",
        ".roo/commands",
        ".codeium/windsurf/global_workflows",
    ] {
        let rel = format!("{dir}/ship.md");
        assert!(f.is_symlink(&rel), "{rel} should be a symlink");
        assert_eq!(
            f.resolves_to(&rel),
            std::fs::canonicalize(f.commons().join("commands/ship.md")).unwrap()
        );
    }
}

#[test]
fn commands_do_not_reach_agents_that_need_rendering_or_have_no_surface() {
    let f = machine();
    f.commons_file("commands/ship.md", "ship\n");

    f.run(&["sync"]).assert_clean();

    // Gemini takes TOML, so it is a render target, not a fan-out one.
    assert!(!f.present(".gemini/commands/ship.md"));
    // pi has no user-scope command surface at all.
    assert!(!f.exists(".pi/commands"));
    assert!(!f.exists(".pi/agent/commands"));
}

#[test]
fn agents_reach_only_claude_and_opencode() {
    let f = machine();
    f.commons_file("agents/reviewer.md", "---\nname: reviewer\n---\n\nreview\n");

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".claude/agents/reviewer.md"));
    assert!(f.is_symlink(".config/opencode/agents/reviewer.md"));

    // Codex's TOML agent format is deliberately out of scope for v1.
    assert!(!f.exists(".codex/agents"));
    assert!(!f.present(".cursor/agents/reviewer.md"));
}

#[test]
fn every_family_is_reported_separately_in_status() {
    let f = machine();
    f.commons_skill("research");
    f.commons_file("commands/ship.md", "ship\n");
    f.commons_file("agents/reviewer.md", "review\n");

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let families: Vec<&str> = parsed["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["agent"] == "claude")
        .map(|t| t["family"].as_str().unwrap())
        .collect();

    assert!(families.contains(&"skills"));
    assert!(families.contains(&"commands"));
    assert!(families.contains(&"agents"));
}

#[test]
fn a_command_variant_is_preserved_like_any_other() {
    let f = machine();
    f.commons_file("commands/ship.md", "the Commons version\n");
    f.file(".claude/commands/ship.md", "a claude-specific version\n");

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        std::fs::read_to_string(f.path(".claude/commands/ship.md")).unwrap(),
        "a claude-specific version\n"
    );
    assert!(f.is_symlink(".cursor/commands/ship.md"));
}

#[test]
fn a_non_markdown_file_in_a_markdown_family_is_reported_not_synced() {
    let f = machine();
    f.commons_file("commands/notes.txt", "not a command\n");

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stderr_has("notes.txt");
    assert!(!f.present(".claude/commands/notes.txt"));
}
