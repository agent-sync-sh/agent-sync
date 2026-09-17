//! The MCP management surface: targeting, Tweaks, and
//! `mcp list/adopt/remove/enable/disable`.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [".claude", ".codex", ".cursor", ".gemini", ".pi"] {
        f.agent(root);
    }
    f
}

fn one_server(f: &Fixture) {
    f.commons_mcp(r#"{"mcpServers": {"serena": {"type": "stdio", "command": "uvx"}}}"#);
}

fn config(f: &Fixture, body: &str) {
    f.file(".config/agent-sync/agent-sync.toml", body);
}

// ------------------------------------------------------------------ Targeting

#[test]
fn an_allowlist_scopes_a_server_to_the_agents_that_want_it() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena]\nagents = [\"claude\", \"codex\"]\n");

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx"
    );
    assert!(
        f.contents(".codex/config.toml")
            .contains("[mcp_servers.serena]")
    );
    assert!(
        !f.present(".cursor/mcp.json"),
        "an unlisted agent receives nothing"
    );
    assert!(!f.present(".gemini/settings.json"));
}

#[test]
fn no_allowlist_means_every_capable_agent() {
    let f = machine();
    one_server(&f);

    f.run(&["sync"]).assert_clean();

    assert!(f.present(".claude.json"));
    assert!(f.present(".cursor/mcp.json"));
    assert!(f.present(".gemini/settings.json"));
}

#[test]
fn an_allowlist_naming_an_unknown_agent_is_an_error() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena]\nagents = [\"nosuchagent\"]\n");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("nosuchagent");
}

#[test]
fn an_allowlisted_agent_that_is_not_installed_is_simply_skipped() {
    let f = Fixture::new();
    f.agent(".claude");
    one_server(&f);
    config(&f, "[mcp.serena]\nagents = [\"claude\", \"cursor\"]\n");

    f.run(&["sync"]).assert_clean();

    assert!(f.present(".claude.json"));
    assert!(!f.exists(".cursor"), "detection still governs");
}

// --------------------------------------------------------------------- Tweaks

#[test]
fn a_tweak_reaches_one_agent_and_no_other() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena.tweaks.codex]\nstartup_timeout_sec = 20\n");

    f.run(&["sync"]).assert_clean();

    assert!(
        f.contents(".codex/config.toml")
            .contains("startup_timeout_sec = 20"),
        "the Tweak reaches its agent"
    );
    assert!(
        f.json(".claude.json")["mcpServers"]["serena"]
            .get("startup_timeout_sec")
            .is_none(),
        "and no other agent sees it"
    );
}

#[test]
fn a_tweak_can_override_a_rendered_key() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena.tweaks.claude]\ncommand = \"different\"\n");

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "different"
    );
    assert!(
        f.contents(".codex/config.toml")
            .contains("command = \"uvx\"")
    );
}

#[test]
fn tweaks_never_pollute_the_commons() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena.tweaks.codex]\nstartup_timeout_sec = 20\n");

    f.run(&["sync"]).assert_clean();

    let commons = f.contents_of_commons("mcp.json");
    assert!(
        !commons.contains("startup_timeout_sec"),
        "the Commons holds the standard shape only:\n{commons}"
    );
}

#[test]
fn a_tweak_for_an_unknown_agent_is_an_error() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena.tweaks.nosuchagent]\nkey = 1\n");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("nosuchagent");
}

#[test]
fn a_tweaked_server_still_converges() {
    let f = machine();
    one_server(&f);
    config(&f, "[mcp.serena.tweaks.codex]\nstartup_timeout_sec = 20\n");

    f.run(&["sync"]).assert_clean();
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");
    f.run(&["status"]).assert_code(0);
}

// ------------------------------------------------------------------ mcp list

#[test]
fn list_shows_commons_servers_and_their_per_target_state() {
    let f = machine();
    one_server(&f);
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"command": "mine"}}}"#,
    );

    let out = f.run(&["mcp", "list"]);

    // Missing servers are actionable, the same as everywhere else.
    out.assert_code(2)
        .assert_stdout_has("serena")
        .assert_stdout_has("missing")
        .assert_stdout_has("handmade")
        .assert_stdout_has("foreign");
}

#[test]
fn list_is_available_as_json() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();

    let out = f.run(&["mcp", "list", "--json"]);
    out.assert_code(0);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let servers = parsed["servers"].as_array().unwrap();
    let serena = servers.iter().find(|s| s["name"] == "serena").unwrap();
    assert_eq!(serena["managed"], true);
    let targets = serena["targets"].as_array().unwrap();
    assert!(
        targets
            .iter()
            .any(|t| t["agent"] == "claude" && t["state"] == "managed")
    );
}

#[test]
fn list_without_a_commons_file_says_so_rather_than_failing() {
    let f = machine();

    f.run(&["mcp", "list"])
        .assert_code(0)
        .assert_stdout_has("no MCP servers");
}

// ---------------------------------------------------------------- mcp remove

#[test]
fn remove_deletes_the_server_from_the_commons_and_every_target() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();

    let out = f.run(&["mcp", "remove", "serena"]);

    out.assert_clean().assert_stdout_has("serena");
    assert!(
        !f.contents_of_commons("mcp.json").contains("serena"),
        "gone from the Commons"
    );
    assert!(f.json(".claude.json")["mcpServers"].get("serena").is_none());
    assert!(!f.contents(".codex/config.toml").contains("serena"));
    assert!(
        f.json(".cursor/mcp.json")["mcpServers"]
            .get("serena")
            .is_none()
    );
}

#[test]
fn remove_leaves_foreign_servers_and_other_keys_alone() {
    let f = machine();
    one_server(&f);
    f.file(
        ".claude.json",
        r#"{"numStartups": 9, "mcpServers": {"handmade": {"command": "mine"}}}"#,
    );
    f.run(&["sync"]).assert_clean();

    f.run(&["mcp", "remove", "serena"]).assert_clean();

    let config = f.json(".claude.json");
    assert_eq!(config["numStartups"], 9);
    assert_eq!(config["mcpServers"]["handmade"]["command"], "mine");
}

#[test]
fn remove_preserves_the_order_of_the_remaining_keys() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"b": {"command": "x"}}}"#);
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"a": {"command": "1"}, "b": {"command": "2"}, "c": {"command": "3"}, "d": {"command": "4"}}}"#,
    );
    f.run(&["sync"]).assert_clean();

    f.run(&["mcp", "remove", "b"]).assert_clean();

    let text = f.contents(".claude.json");
    let (a, c, d) = (
        text.find("\"a\"").unwrap(),
        text.find("\"c\"").unwrap(),
        text.find("\"d\"").unwrap(),
    );
    assert!(a < c && c < d, "removal reordered the map:\n{text}");
}

#[test]
fn removing_a_server_that_is_not_in_the_commons_is_an_error() {
    let f = machine();
    one_server(&f);

    f.run(&["mcp", "remove", "ghost"])
        .assert_code(1)
        .assert_stderr_has("ghost");
}

#[test]
fn a_removed_server_stays_removed_after_a_sync() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();
    f.run(&["mcp", "remove", "serena"]).assert_clean();

    f.run(&["sync"]).assert_clean();

    assert!(f.json(".claude.json")["mcpServers"].get("serena").is_none());
}

// ----------------------------------------------------------------- mcp adopt

#[test]
fn adopt_takes_a_native_claude_entry_into_the_commons() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"type": "stdio", "command": "mine", "args": ["-x"]}}}"#,
    );

    let out = f.run(&["mcp", "adopt", "claude/handmade"]);

    out.assert_clean().assert_stdout_has("handmade");
    let commons: serde_json::Value =
        serde_json::from_str(&f.contents_of_commons("mcp.json")).unwrap();
    assert_eq!(commons["mcpServers"]["handmade"]["command"], "mine");
    assert_eq!(commons["mcpServers"]["handmade"]["args"][0], "-x");
}

#[test]
fn adopting_from_claude_then_syncing_changes_nothing() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"type": "stdio", "command": "mine", "args": ["-x"]}}}"#,
    );
    f.run(&["mcp", "adopt", "claude/handmade"]).assert_clean();

    let before = f.contents(".claude.json");
    let out = f.run(&["sync"]);

    out.assert_clean();
    assert_eq!(
        before,
        f.contents(".claude.json"),
        "an adopted entry must already be exactly what sync would write"
    );
}

#[test]
fn adopting_from_codex_recovers_the_canonical_shape_and_tweaks() {
    let f = machine();
    f.file(
        ".codex/config.toml",
        "[mcp_servers.native]\ncommand = \"c\"\nargs = [\"-y\"]\nstartup_timeout_sec = 30\n",
    );

    let out = f.run(&["mcp", "adopt", "codex/native"]);

    out.assert_clean();
    let commons: serde_json::Value =
        serde_json::from_str(&f.contents_of_commons("mcp.json")).unwrap();
    assert_eq!(commons["mcpServers"]["native"]["command"], "c");
    assert!(
        commons["mcpServers"]["native"]
            .get("startup_timeout_sec")
            .is_none(),
        "a Codex-only knob does not belong in the pure Commons shape"
    );
    let config = f.contents(".config/agent-sync/agent-sync.toml");
    assert!(
        config.contains("startup_timeout_sec"),
        "it becomes a Tweak instead:\n{config}"
    );
}

#[test]
fn adopting_from_codex_then_syncing_changes_nothing() {
    let f = machine();
    f.file(
        ".codex/config.toml",
        "[mcp_servers.native]\ncommand = \"c\"\nargs = [\"-y\"]\nstartup_timeout_sec = 30\n",
    );
    f.run(&["mcp", "adopt", "codex/native"]).assert_clean();
    let before = f.contents(".codex/config.toml");

    f.run(&["sync"]).assert_clean();

    assert_eq!(before, f.contents(".codex/config.toml"));
}

#[test]
fn adoption_reports_what_it_could_not_represent() {
    let f = machine();
    // Codex has one remote transport, so http and sse are indistinguishable.
    f.file(
        ".codex/config.toml",
        "[mcp_servers.remote]\nurl = \"https://x.test\"\n",
    );

    let out = f.run(&["mcp", "adopt", "codex/remote"]);

    out.assert_clean();
    assert!(
        out.stdout.contains("transport") || out.stderr.contains("transport"),
        "an ambiguous transport must be reported, not guessed silently\nstdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn adopting_a_server_the_commons_already_has_identically_is_a_no_op() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"serena": {"type": "stdio", "command": "uvx"}}}"#);
    f.run(&["sync"]).assert_clean();

    f.run(&["mcp", "adopt", "claude/serena"])
        .assert_clean()
        .assert_stdout_has("already");
}

#[test]
fn adopting_a_server_that_differs_from_the_commons_is_refused() {
    let f = machine();
    one_server(&f);
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"serena": {"type": "stdio", "command": "tampered"}}}"#,
    );
    let before = f.contents_of_commons("mcp.json");

    let out = f.run(&["mcp", "adopt", "claude/serena"]);

    out.assert_code(1).assert_stderr_has("differs");
    assert_eq!(before, f.contents_of_commons("mcp.json"));
}

#[test]
fn adopt_needs_a_real_agent_and_a_real_server() {
    let f = machine();
    f.file(".claude.json", r#"{"mcpServers": {}}"#);

    f.run(&["mcp", "adopt", "nosuchagent/x"])
        .assert_code(1)
        .assert_stderr_has("nosuchagent");
    f.run(&["mcp", "adopt", "claude/ghost"])
        .assert_code(1)
        .assert_stderr_has("ghost");
    f.run(&["mcp", "adopt", "claude"])
        .assert_code(1)
        .assert_stderr_has("agent/server");
}

#[test]
fn adopt_all_takes_every_foreign_server_it_finds() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"one": {"command": "a"}, "two": {"command": "b"}}}"#,
    );

    f.run(&["mcp", "adopt", "--all"]).assert_clean();

    let commons: serde_json::Value =
        serde_json::from_str(&f.contents_of_commons("mcp.json")).unwrap();
    assert_eq!(commons["mcpServers"]["one"]["command"], "a");
    assert_eq!(commons["mcpServers"]["two"]["command"], "b");
}

#[test]
fn adoption_never_prints_a_secret_it_found() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "env": {"KEY": "AGENT-SYNC-CANARY-adopt"}}}}"#,
    );

    let out = f.run(&["mcp", "adopt", "claude/api"]);

    out.assert_clean()
        .assert_no_output_contains("AGENT-SYNC-CANARY-adopt");
}

// ------------------------------------------------ mcp enable | disable

#[test]
fn disable_removes_the_renders_and_keeps_the_definition() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();

    let out = f.run(&["mcp", "disable", "serena"]);

    out.assert_clean()
        .assert_stdout_has("removed serena from claude")
        .assert_stdout_has("disabled serena — definition kept; enable restores it everywhere");
    assert!(f.json(".claude.json")["mcpServers"].get("serena").is_none());
    assert!(!f.contents(".codex/config.toml").contains("serena"));
    assert!(
        f.json(".cursor/mcp.json")["mcpServers"]
            .get("serena")
            .is_none()
    );

    let commons: serde_json::Value =
        serde_json::from_str(&f.contents_of_commons("mcp.json")).unwrap();
    assert_eq!(
        commons["mcpServers"]["serena"]["command"], "uvx",
        "the definition is kept"
    );
    assert_eq!(commons["mcpServers"]["serena"]["disabled"], true);
}

#[test]
fn enable_restores_everywhere_and_clears_the_key() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();
    f.run(&["mcp", "disable", "serena"]).assert_clean();

    let out = f.run(&["mcp", "enable", "serena"]);

    out.assert_clean()
        .assert_stdout_has("restored serena to claude")
        .assert_stdout_has("enabled serena");
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx"
    );
    assert!(
        f.contents(".codex/config.toml")
            .contains("[mcp_servers.serena]")
    );

    let commons: serde_json::Value =
        serde_json::from_str(&f.contents_of_commons("mcp.json")).unwrap();
    assert!(
        commons["mcpServers"]["serena"].get("disabled").is_none(),
        "the key is removed entirely, not set false"
    );
}

#[test]
fn disabling_or_enabling_an_unknown_server_is_an_error() {
    let f = machine();
    one_server(&f);
    let before = f.contents_of_commons("mcp.json");

    f.run(&["mcp", "disable", "ghost"])
        .assert_code(1)
        .assert_stderr_has("ghost");
    f.run(&["mcp", "enable", "ghost"])
        .assert_code(1)
        .assert_stderr_has("ghost");
    assert_eq!(before, f.contents_of_commons("mcp.json"));
}

#[test]
fn a_second_disable_or_enable_is_a_no_op() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();

    f.run(&["mcp", "disable", "serena"]).assert_clean();
    f.run(&["mcp", "disable", "serena"])
        .assert_clean()
        .assert_stdout_has("serena is already disabled");

    f.run(&["mcp", "enable", "serena"]).assert_clean();
    f.run(&["mcp", "enable", "serena"])
        .assert_clean()
        .assert_stdout_has("serena is already enabled");
}

#[test]
fn a_hand_set_disabled_key_behaves_like_the_verb_on_the_next_sync() {
    let f = machine();
    one_server(&f);
    f.run(&["sync"]).assert_clean();

    // The file is the interface; the verb is sugar.
    f.commons_mcp(
        r#"{"mcpServers": {"serena": {"type": "stdio", "command": "uvx", "disabled": true}}}"#,
    );

    f.run(&["status"]).assert_code(2);
    f.run(&["sync"]).assert_clean();

    assert!(f.json(".claude.json")["mcpServers"].get("serena").is_none());
    assert!(!f.contents(".codex/config.toml").contains("serena"));
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");
    f.run(&["status"]).assert_code(0);
}

#[test]
fn a_disabled_server_is_neither_written_nor_counted_as_drift() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"serena": {"command": "uvx", "disabled": true}}}"#);

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert!(
        !f.present(".claude.json"),
        "a Disabled server renders nowhere"
    );
    assert!(!f.present(".cursor/mcp.json"));
    f.run(&["status"]).assert_code(0);
}

#[test]
fn list_marks_a_disabled_server() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"serena": {"command": "uvx", "disabled": true}}}"#);

    f.run(&["mcp", "list"])
        .assert_code(0)
        .assert_stdout_has("serena  (disabled)");

    let out = f.run(&["mcp", "list", "--json"]);
    out.assert_code(0);
    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let serena = parsed["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "serena")
        .unwrap();
    assert_eq!(serena["disabled"], true);
    assert_eq!(serena["managed"], true);
}

#[test]
fn foreign_servers_and_other_keys_survive_both_verbs_byte_for_byte() {
    let f = machine();
    one_server(&f);
    f.file(
        ".claude.json",
        r#"{"numStartups": 9, "mcpServers": {"handmade": {"command": "mine"}}}"#,
    );
    f.run(&["sync"]).assert_clean();
    let claude_before = f.contents(".claude.json");

    f.run(&["mcp", "disable", "serena"]).assert_clean();

    let config = f.json(".claude.json");
    assert_eq!(config["numStartups"], 9);
    assert_eq!(config["mcpServers"]["handmade"]["command"], "mine");

    f.run(&["mcp", "enable", "serena"]).assert_clean();

    assert_eq!(
        claude_before,
        f.contents(".claude.json"),
        "a disable/enable round trip must put the file back exactly"
    );
}

#[test]
fn a_non_boolean_disabled_value_is_a_named_survey_problem() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"serena": {"command": "uvx", "disabled": "yes"}}}"#);

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("serena")
        .assert_stderr_has("`disabled` must be true or false");
    f.run(&["status"]).assert_code(1);
    f.run(&["mcp", "list"]).assert_code(1);
}

#[test]
fn the_disabled_key_never_reaches_a_rendered_config() {
    let f = machine();
    // An explicit false renders like an absent key — and stays agent-sync's.
    f.commons_mcp(r#"{"mcpServers": {"serena": {"command": "uvx", "disabled": false}}}"#);

    f.run(&["sync"]).assert_clean();

    assert!(
        f.json(".claude.json")["mcpServers"]["serena"]
            .get("disabled")
            .is_none(),
        "the standard dialect must not pass the key through"
    );
    assert!(!f.contents(".codex/config.toml").contains("disabled"));
    assert!(
        f.json(".cursor/mcp.json")["mcpServers"]["serena"]
            .get("disabled")
            .is_none()
    );
    assert!(!f.contents(".gemini/settings.json").contains("disabled"));
}

// ------------------------------------------------- Regressions from review

#[test]
fn a_tweak_toml_cannot_hold_aborts_the_adoption() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "matrix": [{"a": 1}]}}}"#,
    );

    let out = f.run(&["mcp", "adopt", "claude/api"]);

    out.assert_code(1);
    assert!(
        !f.commons().join("mcp.json").exists(),
        "a Tweak that cannot be stored must not be silently dropped"
    );
}

#[test]
fn a_refused_agent_never_lands_in_the_allowlist() {
    let f = machine();
    // The same name, two agents, different definitions.
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"x": {"type": "stdio", "command": "a"}}}"#,
    );
    f.file(".codex/config.toml", "[mcp_servers.x]\ncommand = \"b\"\n");
    let codex_before = f.contents(".codex/config.toml");

    let out = f.run(&["mcp", "adopt", "--all"]);

    out.assert_code(1);
    let config = f.contents(".config/agent-sync/agent-sync.toml");
    assert!(
        !config.contains("codex"),
        "the refused agent must not be allowlisted:\n{config}"
    );

    // And the refused agent's own config survives the next sync untouched.
    f.run(&["sync"]);
    assert_eq!(codex_before, f.contents(".codex/config.toml"));
}

#[test]
fn an_adoption_that_would_not_survive_a_sync_is_refused() {
    let f = machine();
    // OpenCode takes a command array; a bare string cannot round-trip.
    f.agent(".config/opencode");
    f.file(
        ".config/opencode/opencode.json",
        r#"{"mcp": {"x": {"type": "local", "command": "bunx", "environment": {}}}}"#,
    );
    let before = f.contents(".config/opencode/opencode.json");

    let out = f.run(&["mcp", "adopt", "opencode/x"]);

    if out.code == 0 {
        // If it claims success, the claim must hold.
        f.run(&["sync"]).assert_clean();
        assert_eq!(before, f.contents(".config/opencode/opencode.json"));
    } else {
        assert!(!f.commons().join("mcp.json").exists());
    }
}

#[test]
fn a_literal_credential_taken_into_the_commons_is_called_out() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "env": {"API_TOKEN": "sk-literal"}}}}"#,
    );

    let out = f.run(&["mcp", "adopt", "claude/api"]);

    out.assert_clean().assert_stderr_has("env.API_TOKEN");
    // The warning names the key, never the value.
    out.assert_no_output_contains("sk-literal");
}

#[test]
fn an_env_reference_in_an_agent_config_adopts_cleanly() {
    let f = machine();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "env": {"K": "${env:TOKEN}"}}}}"#,
    );

    // A literal reference must compare equal to itself, not read as unfaithful.
    f.run_with_env(&["mcp", "adopt", "claude/api"], &[("TOKEN", "v")])
        .assert_clean();
}

#[test]
fn a_hand_written_config_survives_an_adoption() {
    let f = machine();
    f.file(
        ".config/agent-sync/agent-sync.toml",
        "# my settings\n[targets]\ncursor = false\n\n# servers\n[mcp.other]\nagents = [\"claude\"]\n",
    );
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "cwd": "/tmp"}}}"#,
    );

    f.run(&["mcp", "adopt", "claude/api"]).assert_clean();

    let config = f.contents(".config/agent-sync/agent-sync.toml");
    assert!(config.contains("# my settings"), "got:\n{config}");
    assert!(config.contains("cursor = false"), "got:\n{config}");
    assert!(config.contains("[mcp.other]"), "got:\n{config}");
    assert!(config.contains("[mcp.api"), "got:\n{config}");
}

#[test]
fn a_malformed_tool_config_never_quotes_its_own_contents() {
    let f = machine();
    one_server(&f);
    f.file(
        ".config/agent-sync/agent-sync.toml",
        "[mcp.api.tweaks.claude]\nsecret = \"CANARY-cfg-leak\" oops\n",
    );

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_no_output_contains("CANARY-cfg-leak");
}

#[test]
fn a_non_table_mcp_section_is_refused_rather_than_ignored() {
    let f = machine();
    one_server(&f);
    f.file(".config/agent-sync/agent-sync.toml", "mcp = 5\n");

    f.run(&["sync"]).assert_code(1).assert_stderr_has("mcp");
}
