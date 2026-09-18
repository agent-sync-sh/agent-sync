//! The instructions family — one Commons `AGENTS.md`, four mechanisms, and a
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
    // What claude-mem does to an instructions file it also writes.
    f.file(".codex/AGENTS.md", "<claude-mem-context>\n");

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("conflict");
    assert_eq!(
        std::fs::read_to_string(f.path(".codex/AGENTS.md")).unwrap(),
        "<claude-mem-context>\n",
        "another tool's file must never be overwritten"
    );
}

#[test]
fn a_conflict_names_the_remedy() {
    let f = machine();
    f.file(".codex/AGENTS.md", "someone else's file\n");

    f.run(&["status"]).assert_stdout_has("move or merge it");
}

#[test]
fn a_conflict_does_not_keep_the_machine_permanently_dirty() {
    let f = machine();
    f.file(".codex/AGENTS.md", "someone else's file\n");
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
    f.file(".codex/AGENTS.md", "<claude-mem-context>\nstuff\n");

    f.run(&["status"]).assert_stdout_has("owned by claude-mem");
}

// ------------------------------------------------------------ include-entry
//
// opencode and Gemini list the instruction files they load in their own config,
// and another tool rewrites their instructions file on disk — so a symlink
// there would carry that tool's writes into the Commons (ADR-0008). One entry
// naming the Commons in the list is the mechanic instead.

fn include_machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".config/opencode");
    f.agent(".gemini");
    f.commons_file("AGENTS.md", "# shared instructions\n");
    f
}

#[test]
fn opencode_gets_an_entry_in_its_instructions_list_not_a_symlink() {
    let f = include_machine();
    f.file(
        ".config/opencode/opencode.json",
        "{\"$schema\": \"https://opencode.ai/config.json\", \"model\": \"x\"}\n",
    );

    f.run(&["sync"]).assert_clean();

    let doc = f.json(".config/opencode/opencode.json");
    assert_eq!(
        doc["instructions"],
        serde_json::json!(["~/.agents/AGENTS.md"])
    );
    assert_eq!(doc["model"], "x", "the user's own keys survive");
    assert!(
        !f.present(".config/opencode/AGENTS.md"),
        "no symlink may be placed where another tool writes"
    );
}

#[test]
fn gemini_gets_an_entry_after_its_own_file_never_before_it() {
    let f = include_machine();

    f.run(&["sync"]).assert_clean();

    // Element 0 is where Gemini writes memories; the Commons must not be it.
    let doc = f.json(".gemini/settings.json");
    assert_eq!(
        doc["context"]["fileName"],
        serde_json::json!(["GEMINI.md", "../.agents/AGENTS.md"])
    );
    assert!(!f.present(".gemini/GEMINI.md"));
}

#[test]
fn an_absent_config_file_is_created_with_just_the_entry() {
    let f = include_machine();

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.json(".config/opencode/opencode.json"),
        serde_json::json!({"instructions": ["~/.agents/AGENTS.md"]})
    );
}

#[test]
fn an_entry_the_user_wrote_in_another_spelling_counts_and_is_not_rewritten() {
    let f = include_machine();
    // The fleet wrote absolute paths by hand before this mechanic existed.
    let absolute = common::slashed(&f.commons().join("AGENTS.md"));
    let body = format!("{{\"instructions\": [\"{absolute}\"]}}\n");
    f.file(".config/opencode/opencode.json", &body);
    // Gemini: a string where an array is allowed, already naming the file.
    f.file(
        ".gemini/settings.json",
        "{\"context\": {\"fileName\": [\"GEMINI.md\", \"../.agents/AGENTS.md\"]}}\n",
    );

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(f.contents(".config/opencode/opencode.json"), body);
    f.run(&["status"])
        .assert_code(0)
        .assert_stdout_has("imported")
        .assert_stdout_has("included via");
}

#[test]
fn the_entry_is_added_only_once() {
    let f = include_machine();
    f.run(&["sync"]).assert_clean();
    let after_first = f.tree();
    let first = f.contents(".gemini/settings.json");

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(after_first, f.tree());
    assert_eq!(f.contents(".gemini/settings.json"), first);
}

#[test]
fn a_string_valued_gemini_file_name_is_promoted_to_a_list() {
    let f = include_machine();
    f.file(
        ".gemini/settings.json",
        "{\"context\": {\"fileName\": \"CONTEXT.md\"}}\n",
    );

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.json(".gemini/settings.json")["context"]["fileName"],
        serde_json::json!(["CONTEXT.md", "../.agents/AGENTS.md"]),
        "the user's own name keeps element 0"
    );
}

#[test]
fn the_commons_first_in_geminis_list_is_a_conflict_not_a_reorder() {
    let f = include_machine();
    let body = "{\"context\": {\"fileName\": [\"../.agents/AGENTS.md\", \"GEMINI.md\"]}}\n";
    f.file(".gemini/settings.json", body);

    f.run(&["sync"]).assert_clean();

    // A conflict is the user's decision, so it is reported but not actionable.
    f.run(&["status"])
        .assert_code(0)
        .assert_stdout_has("conflict")
        .assert_stdout_has("writes its own memories");
    assert_eq!(f.contents(".gemini/settings.json"), body, "never reordered");
}

#[test]
fn malformed_json_is_a_conflict_and_the_file_is_left_alone() {
    let f = include_machine();
    f.file(".gemini/settings.json", "{ not json\n");
    f.file(".config/opencode/opencode.json", "{\"instructions\": 42}\n");

    let out = f.run(&["sync"]);

    out.assert_clean()
        .assert_stdout_has("not valid JSON")
        .assert_stdout_has("is not a list");
    assert_eq!(f.contents(".gemini/settings.json"), "{ not json\n");
    assert_eq!(
        f.contents(".config/opencode/opencode.json"),
        "{\"instructions\": 42}\n"
    );
}

#[test]
fn a_legacy_symlink_of_ours_is_removed_and_a_real_file_is_left_alone() {
    let f = include_machine();
    // What a pre-ADR-0008 registry put at opencode's path…
    f.symlink(".config/opencode/AGENTS.md", "../../.agents/AGENTS.md");
    // …and what claude-mem leaves at Gemini's, which is no longer a destination.
    f.file(".gemini/GEMINI.md", "<claude-mem-context>\n");

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("legacy");
    assert!(
        !f.present(".config/opencode/AGENTS.md"),
        "our symlink is the write-through hazard — removed"
    );
    assert_eq!(f.contents(".gemini/GEMINI.md"), "<claude-mem-context>\n");
    f.run(&["status"])
        .assert_code(0)
        .assert_stdout_lacks("conflict");
}

#[test]
fn a_foreign_symlink_at_the_legacy_path_is_not_ours_to_remove() {
    let f = include_machine();
    f.symlink(".config/opencode/AGENTS.md", "../../elsewhere/AGENTS.md");

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.link_text(".config/opencode/AGENTS.md"),
        "../../elsewhere/AGENTS.md"
    );
}

#[test]
fn status_json_reports_include_entry_agents_with_the_import_states() {
    let f = include_machine();

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);
    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = parsed["instructions"].as_array().unwrap();
    let gemini = entries.iter().find(|e| e["agent"] == "gemini").unwrap();
    assert_eq!(gemini["state"], "import");
    assert_eq!(gemini["actionable"], true);

    f.run(&["sync"]).assert_clean();

    let out = f.run(&["status", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = parsed["instructions"].as_array().unwrap();
    let gemini = entries.iter().find(|e| e["agent"] == "gemini").unwrap();
    assert_eq!(gemini["state"], "imported");
}

#[test]
fn doctor_names_an_import_line_in_a_file_whose_agent_ignores_it() {
    let f = include_machine();
    f.agent(".codex");
    // The trap: the line Claude honors, written where nobody expands it.
    f.file(".codex/AGENTS.md", "# codex notes\n@~/.agents/AGENTS.md\n");
    f.file(".gemini/GEMINI.md", "@~/.agents/AGENTS.md\n");

    f.run(&["doctor"])
        .assert_code(0)
        .assert_stderr_has(".codex/AGENTS.md")
        .assert_stderr_has(".gemini/GEMINI.md")
        .assert_stderr_has("silently ignored");
}
