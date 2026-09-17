//! The hooks family: command-hooks declared once, merged into each agent's
//! native hook arrays under element identity.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [".claude", ".codex", ".gemini", ".cursor", ".pi"] {
        f.agent(root);
    }
    f
}

fn one_hook(f: &Fixture) {
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\nmatcher = \"*\"\ncommand = \"notify session\"\ntimeout = 10\n",
    );
}

/// Every hook object anywhere under one event, flattened.
fn hooks_at(f: &Fixture, rel: &str, root: &str, event: &str) -> Vec<serde_json::Value> {
    let config = f.json(rel);
    config[root][event]
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

#[test]
fn a_commons_hook_reaches_every_agent_that_takes_hooks() {
    let f = machine();
    one_hook(&f);

    f.run(&["sync"]).assert_clean();

    for (rel, event) in [
        (".claude/settings.json", "SessionStart"),
        (".codex/hooks.json", "SessionStart"),
        (".gemini/settings.json", "SessionStart"),
    ] {
        let hooks = hooks_at(&f, rel, "hooks", event);
        assert_eq!(hooks.len(), 1, "{rel} should have one hook: {hooks:?}");
        assert_eq!(hooks[0]["command"], "notify session", "{rel}");
        assert_eq!(hooks[0]["type"], "command", "{rel}");
        assert_eq!(hooks[0]["timeout"], 10, "{rel}");
    }
}

#[test]
fn agents_without_a_hook_surface_get_nothing() {
    let f = machine();
    one_hook(&f);

    f.run(&["sync"]).assert_clean();

    assert!(!f.present(".cursor/hooks.json"));
    assert!(!f.present(".pi/hooks.json"));
}

#[test]
fn other_settings_in_the_same_file_survive() {
    let f = machine();
    one_hook(&f);
    f.file(
        ".claude/settings.json",
        r#"{"model": "opus", "permissions": {"defaultMode": "auto"}, "theme": "dark"}"#,
    );

    f.run(&["sync"]).assert_clean();

    let config = f.json(".claude/settings.json");
    assert_eq!(config["model"], "opus");
    assert_eq!(config["permissions"]["defaultMode"], "auto");
    assert_eq!(config["theme"], "dark");
}

#[test]
fn a_foreign_hook_in_the_same_event_survives_byte_for_byte() {
    let f = machine();
    one_hook(&f);
    // What another tool leaves in the same event array.
    f.file(
        ".claude/settings.json",
        r#"{"hooks": {"SessionStart": [{"matcher": "*", "hooks": [
             {"type": "command", "command": "claude-mem context", "timeout": 10000}]}]}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    let foreign = hooks
        .iter()
        .find(|h| h["command"] == "claude-mem context")
        .expect("the other tool's hook must survive");
    assert_eq!(foreign["timeout"], 10000, "and survive unchanged");
    assert!(
        hooks.iter().any(|h| h["command"] == "notify session"),
        "ours lands alongside it"
    );
}

#[test]
fn our_hook_is_updated_in_place_rather_than_duplicated() {
    let f = machine();
    one_hook(&f);
    f.run(&["sync"]).assert_clean();

    // The Commons changes the timeout.
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\nmatcher = \"*\"\ncommand = \"notify session\"\ntimeout = 99\n",
    );
    f.run(&["sync"]).assert_clean();

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    assert_eq!(hooks.len(), 1, "no duplicate was appended: {hooks:?}");
    assert_eq!(hooks[0]["timeout"], 99, "the Commons wins");
}

#[test]
fn a_second_sync_changes_nothing() {
    let f = machine();
    one_hook(&f);
    f.run(&["sync"]).assert_clean();
    let before = f.contents(".claude/settings.json");

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(before, f.contents(".claude/settings.json"));
}

#[test]
fn codex_trust_hashes_are_never_touched() {
    let f = machine();
    one_hook(&f);
    // Hook definitions live in hooks.json; trust lives in config.toml. Writing
    // one must never reach the other, or agent-sync would be approving code
    // execution on the user's behalf.
    let trust = "[features]\nhooks = true\n\n[hooks.state]\n\n\
                 [hooks.state.\"/home/u/.codex/hooks.json:session_start:0:0\"]\n\
                 trusted_hash = \"sha256:deadbeef\"\n";
    f.file(".codex/config.toml", trust);

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.contents(".codex/config.toml"),
        trust,
        "config.toml holds the trust hashes and must be byte-identical"
    );
    assert!(
        f.present(".codex/hooks.json"),
        "the hook itself was written"
    );
}

#[test]
fn a_hook_removed_from_the_commons_is_left_behind_and_reported() {
    let f = machine();
    one_hook(&f);
    f.run(&["sync"]).assert_clean();
    std::fs::remove_file(f.commons().join("hooks/SessionStart.toml")).unwrap();

    f.run(&["sync"]).assert_clean();

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    assert_eq!(
        hooks.len(),
        1,
        "without state, a vanished Commons hook cannot be told from a hand-added one"
    );
    f.run(&["status"]).assert_stdout_has("foreign");
}

#[test]
fn gemini_gets_its_own_event_names() {
    let f = machine();
    f.commons_file("hooks/PreToolUse.toml", "[[hook]]\ncommand = \"guard\"\n");

    f.run(&["sync"]).assert_clean();

    // Claude and Codex share a vocabulary; Gemini calls this one BeforeTool.
    assert_eq!(
        hooks_at(&f, ".claude/settings.json", "hooks", "PreToolUse").len(),
        1
    );
    assert_eq!(
        hooks_at(&f, ".gemini/settings.json", "hooks", "BeforeTool").len(),
        1,
        "Gemini's name for the same moment"
    );
    assert!(
        f.json(".gemini/settings.json")["hooks"]
            .get("PreToolUse")
            .is_none(),
        "and not under Claude's name"
    );
}

#[test]
fn an_event_an_agent_does_not_have_is_skipped_and_reported() {
    let f = machine();
    f.commons_file(
        "hooks/UserPromptSubmit.toml",
        "[[hook]]\ncommand = \"record\"\n",
    );

    let out = f.run(&["sync"]);

    out.assert_clean();
    assert_eq!(
        hooks_at(&f, ".claude/settings.json", "hooks", "UserPromptSubmit").len(),
        1
    );
    // Gemini has no equivalent moment; saying so beats inventing one.
    assert!(
        !f.present(".gemini/settings.json"),
        "nothing to write means no file is created"
    );
    out.assert_stdout_has("gemini")
        .assert_stdout_has("unsupported");
}

#[test]
fn an_unknown_event_name_is_a_clear_error() {
    let f = machine();
    f.commons_file("hooks/NoSuchMoment.toml", "[[hook]]\ncommand = \"x\"\n");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("NoSuchMoment");
}

#[test]
fn only_command_hooks_are_representable() {
    let f = machine();
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ntype = \"webhook\"\ncommand = \"x\"\n",
    );

    f.run(&["sync"]).assert_code(1).assert_stderr_has("command");
}

#[test]
fn a_hook_without_a_command_is_a_clear_error() {
    let f = machine();
    f.commons_file("hooks/SessionStart.toml", "[[hook]]\nmatcher = \"*\"\n");

    f.run(&["sync"]).assert_code(1).assert_stderr_has("command");
}

#[test]
fn a_malformed_hook_file_is_refused_before_anything_is_written() {
    let f = machine();
    f.commons_file("hooks/SessionStart.toml", "this is not [ toml");

    let out = f.run(&["sync"]);

    out.assert_code(1);
    assert!(!f.present(".claude/settings.json"));
}

#[test]
fn several_hooks_for_one_event_all_arrive() {
    let f = machine();
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"first\"\n\n[[hook]]\ncommand = \"second\"\n",
    );

    f.run(&["sync"]).assert_clean();

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    let commands: Vec<&str> = hooks.iter().filter_map(|h| h["command"].as_str()).collect();
    assert!(commands.contains(&"first") && commands.contains(&"second"));
}

#[test]
fn status_reports_the_hooks_family() {
    let f = machine();
    one_hook(&f);

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = parsed["hooks"].as_array().expect("a hooks array");
    assert!(
        entries
            .iter()
            .any(|e| e["agent"] == "claude" && e["state"] == "missing")
    );
}

#[test]
fn a_hook_command_is_written_verbatim_and_converges() {
    let f = machine();
    // The command IS the identity, so it must not vary with the environment:
    // a resolved command would stop matching the reference that produced it.
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"notify --token ${env:TOKEN}\"\n",
    );

    f.run_with_env(&["sync"], &[("TOKEN", "CANARY-hook-9f2")])
        .assert_clean();

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    assert_eq!(
        hooks[0]["command"], "notify --token ${env:TOKEN}",
        "the reference is passed through, so the secret never reaches the file"
    );

    // And it settles: the same hook is recognised as its own on the next run.
    f.run_with_env(&["sync"], &[("TOKEN", "CANARY-hook-9f2")])
        .assert_clean()
        .assert_stdout_has("up to date");
    f.run_with_env(&["status"], &[("TOKEN", "CANARY-hook-9f2")])
        .assert_code(0);
}

#[test]
fn a_hook_converges_even_when_the_environment_changes() {
    let f = machine();
    f.commons_file(
        "hooks/SessionStart.toml",
        "[[hook]]\ncommand = \"notify --token ${env:TOKEN}\"\n",
    );
    f.run_with_env(&["sync"], &[("TOKEN", "first")])
        .assert_clean();

    // A changed value must not orphan the hook and append a second one.
    f.run_with_env(&["sync"], &[("TOKEN", "second")])
        .assert_clean()
        .assert_stdout_has("up to date");

    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    assert_eq!(hooks.len(), 1, "no orphan was left behind: {hooks:?}");
}

#[test]
fn no_commons_hooks_means_no_hook_work() {
    let f = machine();
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert!(!f.present(".codex/hooks.json"));
}

#[test]
fn a_foreign_hooks_arguments_are_never_printed() {
    let f = machine();
    one_hook(&f);
    // Somebody else's hook, with a credential on its command line.
    f.file(
        ".claude/settings.json",
        r#"{"hooks": {"SessionStart": [{"matcher": "*", "hooks": [
             {"type": "command", "command": "curl -H 'Authorization: sk-SECRET-abc' https://x.test"}]}]}}"#,
    );

    for args in [vec!["sync"], vec!["status"], vec!["status", "--json"]] {
        let out = f.run(&args);
        out.assert_no_output_contains("sk-SECRET-abc");
        // The program is still named, so the report stays useful.
        assert!(
            out.stdout.contains("curl"),
            "the hook should still be identifiable:\n{}",
            out.stdout
        );
    }

    // And it is still there afterwards.
    let hooks = hooks_at(&f, ".claude/settings.json", "hooks", "SessionStart");
    assert!(hooks.iter().any(|h| {
        h["command"]
            .as_str()
            .unwrap_or("")
            .contains("sk-SECRET-abc")
    }));
}

#[test]
fn changing_the_matcher_moves_the_hook() {
    let f = machine();
    f.commons_file(
        "hooks/PreToolUse.toml",
        "[[hook]]\nmatcher = \"Bash\"\ncommand = \"guard\"\n",
    );
    f.run(&["sync"]).assert_clean();

    // A narrowing constraint the user changed must actually take effect.
    f.commons_file(
        "hooks/PreToolUse.toml",
        "[[hook]]\nmatcher = \"Write\"\ncommand = \"guard\"\n",
    );
    f.run(&["sync"]).assert_clean();

    let groups = f.json(".claude/settings.json")["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .clone();
    let matchers: Vec<Option<&str>> = groups.iter().map(|g| g["matcher"].as_str()).collect();
    assert!(
        matchers.contains(&Some("Write")),
        "the hook should now be under Write: {matchers:?}"
    );
    assert_eq!(
        hooks_at(&f, ".claude/settings.json", "hooks", "PreToolUse").len(),
        1,
        "and not left behind under Bash as well"
    );
    assert!(
        !matchers.contains(&Some("Bash")),
        "the emptied group should be gone: {matchers:?}"
    );
}

#[test]
fn a_changed_matcher_is_reported_as_drift() {
    let f = machine();
    f.commons_file(
        "hooks/PreToolUse.toml",
        "[[hook]]\nmatcher = \"Bash\"\ncommand = \"guard\"\n",
    );
    f.run(&["sync"]).assert_clean();
    f.commons_file(
        "hooks/PreToolUse.toml",
        "[[hook]]\nmatcher = \"Write\"\ncommand = \"guard\"\n",
    );

    f.run(&["status"])
        .assert_code(2)
        .assert_stdout_has("drifted");
}

#[test]
fn moving_our_hook_leaves_a_foreign_sibling_where_it_was() {
    let f = machine();
    f.commons_file(
        "hooks/PreToolUse.toml",
        "[[hook]]\nmatcher = \"Write\"\ncommand = \"guard\"\n",
    );
    // Ours starts in a group it shares with somebody else's hook.
    f.file(
        ".claude/settings.json",
        r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
             {"type": "command", "command": "guard"},
             {"type": "command", "command": "theirs"}]}]}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let groups = f.json(".claude/settings.json")["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .clone();
    let bash = groups
        .iter()
        .find(|g| g["matcher"] == "Bash")
        .expect("their group survives");
    let commands: Vec<&str> = bash["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["command"].as_str())
        .collect();
    assert_eq!(commands, vec!["theirs"], "only ours was taken out");
}

#[test]
fn doctor_knows_about_the_hooks_family() {
    let f = machine();
    one_hook(&f);

    f.run(&["doctor"]).assert_clean().assert_stdout_has("hooks");
}

#[test]
fn a_file_hooks_would_skip_is_warned_about() {
    let f = machine();
    one_hook(&f);
    f.commons_file("hooks/notes.md", "not a hook file\n");

    f.run(&["doctor"]).assert_stderr_has("notes.md");
}
