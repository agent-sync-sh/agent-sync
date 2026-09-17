//! Per-agent MCP dialects. One canonical Commons entry, six native shapes.

mod common;

use common::Fixture;

const CANARY: &str = "AGENT-SYNC-CANARY-d9a2c4";

/// Every MCP-capable agent, plus the two that must never be written.
fn machine() -> Fixture {
    let f = Fixture::new();
    for root in [
        ".claude",
        ".codex",
        ".config/opencode",
        ".gemini",
        ".cursor",
        ".cline",
        ".codeium/windsurf",
        ".pi",
        ".omp",
    ] {
        f.agent(root);
    }
    f
}

fn stdio_commons(f: &Fixture) {
    f.commons_mcp(
        r#"{"mcpServers": {"serena": {
             "type": "stdio",
             "command": "uvx",
             "args": ["--from", "serena-agent", "serena"],
             "env": {"LOG": "debug"}}}}"#,
    );
}

// ---------------------------------------------------------------- Codex (TOML)

#[test]
fn codex_gets_toml_tables_not_json() {
    let f = machine();
    stdio_commons(&f);

    f.run(&["sync"]).assert_clean();

    let toml = f.contents(".codex/config.toml");
    assert!(
        toml.contains("[mcp_servers.serena]"),
        "expected a table section, got:\n{toml}"
    );
    assert!(toml.contains("command = \"uvx\""), "got:\n{toml}");
    assert!(
        toml.contains("[mcp_servers.serena.env]"),
        "env belongs in its own sub-table, got:\n{toml}"
    );
    assert!(
        !toml.contains("\"type\""),
        "Codex has no type key, got:\n{toml}"
    );
}

#[test]
fn codex_renames_headers_and_drops_the_command_on_a_remote_server() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"type": "http", "url": "https://example.test",
             "headers": {"X-Key": "abc"}, "command": "ignored"}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let toml = f.contents(".codex/config.toml");
    assert!(
        toml.contains("[mcp_servers.api.http_headers]"),
        "got:\n{toml}"
    );
    assert!(
        toml.contains("url = \"https://example.test\""),
        "got:\n{toml}"
    );
    assert!(
        !toml.contains("command"),
        "a remote server carries no command, got:\n{toml}"
    );
}

#[test]
fn codex_collapses_sse_onto_the_same_shape_as_http() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"a": {"type": "sse", "url": "https://x.test"}}}"#);
    f.run(&["sync"]).assert_clean();
    let sse = f.contents(".codex/config.toml");

    let g = machine();
    g.commons_mcp(r#"{"mcpServers": {"a": {"type": "http", "url": "https://x.test"}}}"#);
    g.run(&["sync"]).assert_clean();

    assert_eq!(
        sse,
        g.contents(".codex/config.toml"),
        "Codex has one remote transport, so both render identically"
    );
}

#[test]
fn codex_keeps_every_other_line_of_the_users_config() {
    let f = machine();
    stdio_commons(&f);
    let original = r#"# my codex config
model = "gpt-5.3-codex"
model_reasoning_effort = "high"

[projects."/Users/me/work"]
trust_level = "trusted"

[mcp_servers.handwritten]
command = "mine"

[plugins."thing@market"]
enabled = true
"#;
    f.file(".codex/config.toml", original);

    f.run(&["sync"]).assert_clean();

    let after = f.contents(".codex/config.toml");
    for line in [
        "# my codex config",
        "model = \"gpt-5.3-codex\"",
        "model_reasoning_effort = \"high\"",
        "[projects.\"/Users/me/work\"]",
        "trust_level = \"trusted\"",
        "[plugins.\"thing@market\"]",
        "enabled = true",
    ] {
        assert!(after.contains(line), "lost {line:?} from:\n{after}");
    }
    assert!(
        after.contains("[mcp_servers.handwritten]") && after.contains("command = \"mine\""),
        "a Foreign server must survive:\n{after}"
    );
    assert!(after.contains("[mcp_servers.serena]"), "got:\n{after}");
}

#[test]
fn a_second_codex_sync_is_byte_identical() {
    let f = machine();
    stdio_commons(&f);
    f.run(&["sync"]).assert_clean();
    let first = f.contents(".codex/config.toml");

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(first, f.contents(".codex/config.toml"));
}

#[test]
fn a_broken_codex_config_is_reported_and_left_alone() {
    let f = machine();
    stdio_commons(&f);
    f.file(
        ".codex/config.toml",
        "model = \"gpt-5\"\nthis is not [ valid toml\n",
    );

    let out = f.run(&["sync"]);

    out.assert_code(1).assert_stderr_has("config.toml");
    assert!(
        f.contents(".codex/config.toml")
            .contains("not [ valid toml")
    );
    // Healthy agents are still synced.
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx"
    );
}

#[test]
fn a_codex_server_map_of_the_wrong_shape_is_refused() {
    let f = machine();
    stdio_commons(&f);
    f.file(".codex/config.toml", "mcp_servers = 5\n");

    let out = f.run(&["sync"]);

    out.assert_code(1);
    assert_eq!(f.contents(".codex/config.toml"), "mcp_servers = 5\n");
}

// ------------------------------------------------------------------- OpenCode

#[test]
fn opencode_flattens_the_command_and_renames_env() {
    let f = machine();
    stdio_commons(&f);

    f.run(&["sync"]).assert_clean();

    let entry = &f.json(".config/opencode/opencode.json")["mcp"]["serena"];
    assert_eq!(entry["type"], "local");
    assert_eq!(
        entry["command"],
        serde_json::json!(["uvx", "--from", "serena-agent", "serena"]),
        "OpenCode takes one array, executable first"
    );
    assert_eq!(entry["environment"]["LOG"], "debug");
    assert!(entry.get("args").is_none(), "args is folded into command");
    assert!(entry.get("env").is_none(), "env is renamed to environment");
}

#[test]
fn opencode_calls_a_remote_server_remote() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"type": "sse", "url": "https://x.test",
             "headers": {"A": "b"}}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let entry = &f.json(".config/opencode/opencode.json")["mcp"]["api"];
    assert_eq!(entry["type"], "remote");
    assert_eq!(entry["url"], "https://x.test");
    assert_eq!(entry["headers"]["A"], "b");
}

#[test]
fn opencodes_own_config_survives_a_plugin_style_round_trip() {
    let f = machine();
    stdio_commons(&f);
    // The real file: $schema first, then the plugin array a plugin rewrites.
    f.file(
        ".config/opencode/opencode.json",
        "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"plugin\": [\"./plugins/claude-mem.js\", \"oh-my-openagent@latest\"]\n}",
    );

    f.run(&["sync"]).assert_clean();

    let config = f.json(".config/opencode/opencode.json");
    assert_eq!(config["$schema"], "https://opencode.ai/config.json");
    assert_eq!(
        config["plugin"],
        serde_json::json!(["./plugins/claude-mem.js", "oh-my-openagent@latest"])
    );
    assert_eq!(config["mcp"]["serena"]["type"], "local");

    // A plugin then rewrites the file, preserving unknown keys as it does in
    // reality — our entry must survive that.
    let text = f.contents(".config/opencode/opencode.json");
    let mut reparsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    reparsed["plugin"] = serde_json::json!(["oh-my-openagent@latest"]);
    f.file(
        ".config/opencode/opencode.json",
        &serde_json::to_string_pretty(&reparsed).unwrap(),
    );

    f.run(&["sync"]).assert_clean();
    assert_eq!(
        f.json(".config/opencode/opencode.json")["mcp"]["serena"]["type"],
        "local"
    );
}

// --------------------------------------------------------------------- Gemini

#[test]
fn gemini_distinguishes_sse_from_http_by_which_url_key_it_uses() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {
             "streamed": {"type": "http", "url": "https://h.test"},
             "evented": {"type": "sse", "url": "https://s.test"}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let servers = &f.json(".gemini/settings.json")["mcpServers"];
    assert_eq!(servers["streamed"]["httpUrl"], "https://h.test");
    assert!(servers["streamed"].get("url").is_none());
    assert_eq!(servers["evented"]["url"], "https://s.test");
    assert!(servers["evented"].get("httpUrl").is_none());
}

#[test]
fn gemini_keeps_its_other_settings() {
    let f = machine();
    stdio_commons(&f);
    f.file(
        ".gemini/settings.json",
        r#"{"security": {"auth": {"selectedType": "oauth"}}, "ui": {"footer": {"items": []}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let config = f.json(".gemini/settings.json");
    assert_eq!(config["security"]["auth"]["selectedType"], "oauth");
    assert_eq!(config["mcpServers"]["serena"]["command"], "uvx");
    assert!(config["mcpServers"]["serena"].get("httpUrl").is_none());
}

// ------------------------------------------------------------------- Windsurf

#[test]
fn windsurf_uses_its_own_url_key() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"api": {"type": "http", "url": "https://x.test"}}}"#);

    f.run(&["sync"]).assert_clean();

    let entry = &f.json(".codeium/windsurf/mcp_config.json")["mcpServers"]["api"];
    assert_eq!(entry["serverUrl"], "https://x.test");
    assert!(entry.get("url").is_none());
}

#[test]
fn windsurf_leaves_a_stdio_server_alone() {
    let f = machine();
    stdio_commons(&f);

    f.run(&["sync"]).assert_clean();

    let entry = &f.json(".codeium/windsurf/mcp_config.json")["mcpServers"]["serena"];
    assert_eq!(entry["command"], "uvx");
    assert!(entry.get("serverUrl").is_none());
}

// ------------------------------------------------------------- Cursor & Cline

#[test]
fn cursor_and_cline_take_the_standard_shape() {
    let f = machine();
    stdio_commons(&f);

    f.run(&["sync"]).assert_clean();

    for rel in [".cursor/mcp.json", ".cline/mcp.json"] {
        let entry = &f.json(rel)["mcpServers"]["serena"];
        assert_eq!(entry["command"], "uvx", "{rel}");
        assert_eq!(entry["type"], "stdio", "{rel}");
        assert_eq!(entry["env"]["LOG"], "debug", "{rel}");
    }
}

// ------------------------------------------------------------------ Exclusions

#[test]
fn pi_and_oh_my_pi_are_never_written() {
    let f = machine();
    stdio_commons(&f);
    let pi_before: Vec<String> = f
        .tree()
        .into_iter()
        .filter(|e| e.contains(".pi/") || e.contains(".omp/"))
        .collect();

    f.run(&["sync"]).assert_clean();

    let pi_after: Vec<String> = f
        .tree()
        .into_iter()
        .filter(|e| e.contains(".pi/") || e.contains(".omp/"))
        .collect();
    assert_eq!(
        pi_before, pi_after,
        "pi rejects MCP by design and oh-my-pi discovers other agents' config"
    );
}

// ---------------------------------------------------------------- Cross-agent

#[test]
fn an_unmodelled_key_survives_into_every_dialect() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"x": {"command": "c", "timeout": 30, "cwd": "/tmp"}}}"#);

    f.run(&["sync"]).assert_clean();

    assert_eq!(f.json(".claude.json")["mcpServers"]["x"]["timeout"], 30);
    assert_eq!(f.json(".cursor/mcp.json")["mcpServers"]["x"]["cwd"], "/tmp");
    assert_eq!(
        f.json(".config/opencode/opencode.json")["mcp"]["x"]["timeout"],
        30
    );
    assert!(f.contents(".codex/config.toml").contains("timeout = 30"));
}

#[test]
fn one_unset_variable_stops_every_dialect_before_any_file_is_opened() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"a": {"command": "c", "env": {"K": "${env:NOPE}"}}}}"#);

    let out = f.run(&["sync"]);

    out.assert_code(1).assert_stderr_has("NOPE");
    for rel in [
        ".claude.json",
        ".codex/config.toml",
        ".cursor/mcp.json",
        ".gemini/settings.json",
    ] {
        assert!(!f.present(rel), "{rel} should not exist");
    }
}

#[test]
fn no_dialect_ever_prints_a_resolved_secret() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"type": "http", "url": "https://x.test",
             "headers": {"Authorization": "${env:TOKEN}"}}}}"#,
    );

    for args in [
        vec!["status"],
        vec!["sync", "--dry-run"],
        vec!["sync"],
        vec!["status", "--json"],
    ] {
        let out = f.run_with_env(&args, &[("TOKEN", CANARY)]);
        out.assert_no_output_contains(CANARY);
    }

    // The canary really did reach every dialect's file.
    assert!(f.contents(".codex/config.toml").contains(CANARY));
    assert_eq!(
        f.json(".gemini/settings.json")["mcpServers"]["api"]["headers"]["Authorization"],
        CANARY
    );
}

#[test]
fn every_mcp_capable_agent_is_reported_by_status() {
    let f = machine();
    stdio_commons(&f);
    f.run(&["sync"]).assert_clean();

    let out = f.run(&["status", "--json"]);
    out.assert_code(0);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let agents: Vec<&str> = parsed["mcp"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["agent"].as_str().unwrap())
        .collect();

    for expected in [
        "claude", "codex", "opencode", "gemini", "cursor", "cline", "windsurf",
    ] {
        assert!(
            agents.contains(&expected),
            "missing {expected} in {agents:?}"
        );
    }
    assert!(!agents.contains(&"pi"));
    assert!(!agents.contains(&"oh-my-pi"));
}

// ------------------------------------------------- Regressions from review

#[test]
fn a_toml_parse_error_never_quotes_the_offending_line() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"type": "http", "url": "https://x.test",
             "headers": {"Authorization": "${env:TOKEN}"}}}}"#,
    );
    f.run_with_env(&["sync"], &[("TOKEN", CANARY)])
        .assert_clean();

    // The user breaks the syntax on the very line holding the resolved secret.
    let broken = f
        .contents(".codex/config.toml")
        .replace(CANARY, &format!("{CANARY}\" oops"));
    f.file(".codex/config.toml", &broken);

    for args in [
        vec!["status"],
        vec!["status", "--json"],
        vec!["sync", "--dry-run"],
        vec!["sync"],
    ] {
        f.run_with_env(&args, &[("TOKEN", CANARY)])
            .assert_no_output_contains(CANARY);
    }
}

#[test]
fn a_comment_above_a_managed_server_survives() {
    let f = machine();
    stdio_commons(&f);
    f.file(
        ".codex/config.toml",
        "# top of file\n\n# this documents serena\n[mcp_servers.serena]\ncommand = \"old\"\n",
    );

    f.run(&["sync"]).assert_clean();

    let after = f.contents(".codex/config.toml");
    assert!(after.contains("# top of file"), "got:\n{after}");
    assert!(after.contains("# this documents serena"), "got:\n{after}");
    assert!(after.contains("command = \"uvx\""), "got:\n{after}");
}

#[test]
fn a_bare_parent_header_the_user_wrote_is_kept() {
    let f = machine();
    stdio_commons(&f);
    f.file(
        ".codex/config.toml",
        "# ---- MCP ----\n[mcp_servers]\n\n[mcp_servers.serena]\ncommand = \"old\"\n",
    );

    f.run(&["sync"]).assert_clean();

    let after = f.contents(".codex/config.toml");
    assert!(after.contains("# ---- MCP ----"), "got:\n{after}");
    assert!(after.contains("[mcp_servers]"), "got:\n{after}");
}

#[test]
fn a_null_in_the_commons_does_not_leave_codex_drifting_forever() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"x": {"command": "c", "note": null, "env": {"A": null}}}}"#);

    f.run(&["sync"]).assert_clean();
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    f.run(&["status"]).assert_code(0);
}

#[test]
fn a_failed_write_is_never_reported_as_up_to_date() {
    let f = machine();
    // An array of objects has no TOML representation inside a server entry.
    f.commons_mcp(r#"{"mcpServers": {"x": {"command": "c", "matrix": [{"a": 1}]}}}"#);

    let out = f.run(&["sync"]);

    out.assert_code(1)
        .assert_stdout_lacks("Everything is up to date");
}

#[test]
fn a_wrong_typed_server_map_is_reported_rather_than_read_as_empty() {
    let f = machine();
    stdio_commons(&f);
    f.file(".cursor/mcp.json", r#"{"mcpServers": "not an object"}"#);

    let out = f.run(&["status"]);

    out.assert_code(1).assert_stderr_has(".cursor/mcp.json");
    // Every healthy agent is still reported.
    out.assert_stdout_has("claude");
}

#[test]
fn an_integer_too_large_for_toml_is_refused_not_corrupted() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"x": {"command": "c", "big": 18446744073709551615}}}"#);

    let out = f.run(&["sync"]);

    out.assert_code(1);
    assert!(!f.present(".codex/config.toml"));
}

#[test]
fn the_users_line_endings_survive() {
    let f = machine();
    stdio_commons(&f);
    f.file(
        ".codex/config.toml",
        "model = \"gpt-5\"\r\n\r\n[other]\r\nkey = 1\r\n",
    );

    f.run(&["sync"]).assert_clean();

    let after = f.contents(".codex/config.toml");
    assert!(after.contains("\r\n"), "CRLF was rewritten to LF");
    assert!(
        !after.contains("\n\n") || after.contains("\r\n\r\n"),
        "line endings were left mixed:\n{after:?}"
    );
    assert!(after.contains("[mcp_servers.serena]"));
}
