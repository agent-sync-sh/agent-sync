//! MCP servers — the first Rendered family. One Commons file in the standard
//! shape, key-merged into each agent's native config under name identity.

mod common;

use common::Fixture;
use std::os::unix::fs::PermissionsExt;

/// A canary that must never appear in any output stream.
const CANARY: &str = "AGENT-SYNC-CANARY-b7f3e1";

fn machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".claude");
    f
}

fn one_server() -> &'static str {
    r#"{
  "mcpServers": {
    "serena": {
      "type": "stdio",
      "command": "uvx",
      "args": ["--from", "git+https://github.com/oraios/serena", "serena"]
    }
  }
}"#
}

#[test]
fn a_commons_server_reaches_the_agents_config() {
    let f = machine();
    f.commons_mcp(one_server());

    f.run(&["sync"]).assert_clean();

    let config = f.json(".claude.json");
    assert_eq!(config["mcpServers"]["serena"]["command"], "uvx");
    assert_eq!(config["mcpServers"]["serena"]["type"], "stdio");
    assert_eq!(
        config["mcpServers"]["serena"]["args"][0], "--from",
        "argument arrays survive"
    );
}

#[test]
fn a_new_config_file_is_created_private() {
    let f = machine();
    f.commons_mcp(one_server());

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.mode(".claude.json"),
        0o600,
        "a file that will hold resolved secrets must not be world-readable"
    );
}

#[test]
fn everything_else_in_the_config_survives() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{
  "numStartups": 42,
  "oauthAccount": {"accountUuid": "abc-123", "organizationName": "Acme"},
  "cachedFeatures": {"probability": 0.05, "other": 0.25},
  "bigNumber": 9007199254740993,
  "projects": {"/Users/x/repo": {"lastCost": 1.5, "allowedTools": []}},
  "mcpServers": {"other": {"command": "keep-me"}}
}"#,
    );

    f.run(&["sync"]).assert_clean();

    let config = f.json(".claude.json");
    assert_eq!(config["numStartups"], 42);
    assert_eq!(config["oauthAccount"]["organizationName"], "Acme");
    assert_eq!(config["cachedFeatures"]["probability"], 0.05);
    assert_eq!(config["projects"]["/Users/x/repo"]["lastCost"], 1.5);
    assert_eq!(
        config["bigNumber"].to_string(),
        "9007199254740993",
        "a large integer must not be mangled by the round trip"
    );
    assert_eq!(config["mcpServers"]["serena"]["command"], "uvx");
}

#[test]
fn the_order_of_someone_elses_keys_is_preserved() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{"zebra": 1, "apple": 2, "middle": 3, "mcpServers": {}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let text = f.contents(".claude.json");
    let zebra = text.find("zebra").unwrap();
    let apple = text.find("apple").unwrap();
    let middle = text.find("middle").unwrap();
    assert!(
        zebra < apple && apple < middle,
        "keys were reordered, which would churn the user's file: {text}"
    );
}

#[test]
fn a_foreign_server_is_never_touched() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"command": "mine", "args": ["--keep"]}}}"#,
    );

    let out = f.run(&["sync"]);
    out.assert_clean();

    let config = f.json(".claude.json");
    assert_eq!(config["mcpServers"]["handmade"]["command"], "mine");
    assert_eq!(config["mcpServers"]["handmade"]["args"][0], "--keep");
}

#[test]
fn a_foreign_server_is_reported_but_needs_no_action() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"command": "mine"}}}"#,
    );
    f.run(&["sync"]).assert_clean();

    f.run(&["status"])
        .assert_code(0)
        .assert_stdout_has("handmade")
        .assert_stdout_has("foreign");
}

#[test]
fn a_hand_edited_managed_entry_is_restored() {
    let f = machine();
    f.commons_mcp(one_server());
    f.run(&["sync"]).assert_clean();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"serena": {"command": "tampered", "type": "stdio"}}}"#,
    );

    let out = f.run(&["sync"]);

    out.assert_clean();
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx",
        "the Commons wins for a Managed entry"
    );
}

#[test]
fn restoring_a_managed_entry_says_which_keys_changed() {
    let f = machine();
    f.commons_mcp(one_server());
    f.run(&["sync"]).assert_clean();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"serena": {"command": "tampered", "type": "stdio"}}}"#,
    );

    let out = f.run(&["sync"]);

    // Visible, never silent — but by key name, never by value.
    out.assert_stdout_has("serena").assert_stdout_has("command");
}

#[test]
fn a_stale_key_disappears_when_the_commons_no_longer_has_it() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"serena": {"command": "uvx", "type": "stdio", "leftover": "gone"}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    assert!(
        f.json(".claude.json")["mcpServers"]["serena"]
            .get("leftover")
            .is_none(),
        "a Managed entry is replaced wholesale, not deep-merged"
    );
}

#[test]
fn drift_is_actionable_and_a_synced_machine_is_clean() {
    let f = machine();
    f.commons_mcp(one_server());

    f.run(&["status"])
        .assert_code(2)
        .assert_stdout_has("serena");

    f.run(&["sync"]).assert_clean();

    f.run(&["status"]).assert_code(0);
}

#[test]
fn a_second_sync_changes_nothing_at_all() {
    let f = machine();
    f.commons_mcp(one_server());
    f.run(&["sync"]).assert_clean();
    let after_first = f.contents(".claude.json");

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("up to date");
    assert_eq!(
        after_first,
        f.contents(".claude.json"),
        "a second sync must be byte-identical"
    );
}

#[test]
fn env_references_resolve_at_sync() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"type": "http", "url": "https://example.test",
             "headers": {"Authorization": "Bearer ${env:TOKEN}"}}}}"#,
    );

    f.run_with_env(&["sync"], &[("TOKEN", CANARY)])
        .assert_clean();

    assert_eq!(
        f.json(".claude.json")["mcpServers"]["api"]["headers"]["Authorization"],
        format!("Bearer {CANARY}")
    );
}

#[test]
fn an_unset_variable_is_a_clear_error_and_nothing_is_written() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"api": {"command": "x", "env": {"KEY": "${env:MISSING_TOKEN}"}}}}"#,
    );

    let out = f.run(&["sync"]);

    out.assert_code(1).assert_stderr_has("MISSING_TOKEN");
    assert!(
        !f.present(".claude.json"),
        "a partially resolved entry must never be written"
    );
}

#[test]
fn a_resolved_secret_never_appears_in_any_output() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"api": {"command": "x", "env": {"KEY": "${env:TOKEN}"}}}}"#);

    for args in [
        vec!["status"],
        vec!["status", "--json"],
        vec!["sync", "--dry-run"],
        vec!["sync"],
        vec!["doctor"],
        vec!["status"],
    ] {
        let out = f.run_with_env(&args, &[("TOKEN", CANARY)]);
        out.assert_no_output_contains(CANARY);
        // Anti-vacuity: the server itself must be mentioned somewhere, so this
        // is not passing simply because nothing was printed.
        if args[0] != "doctor" {
            out.assert_stdout_has("api");
        }
    }

    // ...and it did land in the file, so the canary was real.
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["api"]["env"]["KEY"],
        CANARY
    );
}

#[test]
fn a_secret_that_drifted_is_not_printed_when_restored() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"api": {"command": "x", "env": {"KEY": "${env:TOKEN}"}}}}"#);
    f.run_with_env(&["sync"], &[("TOKEN", CANARY)])
        .assert_clean();
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"api": {"command": "x", "env": {"KEY": "stale"}}}}"#,
    );

    let out = f.run_with_env(&["sync"], &[("TOKEN", CANARY)]);

    out.assert_no_output_contains(CANARY);
    out.assert_stdout_has("env");
}

#[test]
fn a_symlinked_config_keeps_its_symlink() {
    let f = machine();
    f.commons_mcp(one_server());
    // A dotfiles manager owns the real file elsewhere.
    let real = f.root().join("dotfiles").join("claude.json");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, r#"{"numStartups": 7}"#).unwrap();
    f.symlink(".claude.json", &real.display().to_string());

    f.run(&["sync"]).assert_clean();

    assert!(
        f.is_symlink(".claude.json"),
        "replacing the symlink would break the dotfiles wiring"
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
    assert_eq!(written["mcpServers"]["serena"]["command"], "uvx");
    assert_eq!(written["numStartups"], 7);
}

#[test]
fn agents_without_an_mcp_surface_never_get_a_config() {
    let f = Fixture::new();
    f.agent(".pi");
    f.agent(".omp");
    f.agent(".roo");
    f.commons_mcp(one_server());

    f.run(&["sync"]).assert_clean();

    // pi rejects MCP by design; oh-my-pi discovers other agents' config; Roo's
    // user-scope MCP lives in VS Code storage.
    assert!(!f.present(".pi/config.json"));
    assert!(!f.present(".omp/agent/config.yml"));
    assert!(!f.present(".roo/mcp.json"));
}

#[test]
fn no_commons_file_means_no_mcp_work() {
    let f = machine();
    f.commons_skill("research");
    let before = f.tree();

    f.run(&["sync"]).assert_clean();

    assert!(!f.present(".claude.json"));
    assert_ne!(before, f.tree(), "the skill still synced");
}

#[test]
fn a_malformed_commons_file_is_refused_before_anything_is_written() {
    let f = machine();
    f.commons_mcp("{ this is not json");

    let out = f.run(&["sync"]);

    out.assert_code(1).assert_stderr_has("mcp.json");
    assert!(!f.present(".claude.json"));
}

#[test]
fn a_malformed_agent_config_is_reported_not_overwritten() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(".claude.json", "{ not json at all");

    let out = f.run(&["sync"]);

    out.assert_code(1).assert_stderr_has(".claude.json");
    assert_eq!(
        f.contents(".claude.json"),
        "{ not json at all",
        "an unparseable file is left exactly as found"
    );
    assert!(
        !f.present(".claude.json.agent-sync"),
        "and no scratch file is left beside it"
    );
}

#[test]
fn dry_run_writes_nothing() {
    let f = machine();
    f.commons_mcp(one_server());
    let before = f.tree();

    f.run(&["sync", "--dry-run"])
        .assert_clean()
        .assert_stdout_has("serena");

    assert_eq!(before, f.tree());
    assert!(!f.present(".claude.json"));
}

#[test]
fn no_temporary_files_are_left_behind() {
    let f = machine();
    f.commons_mcp(one_server());

    f.run(&["sync"]).assert_clean();

    let stray: Vec<String> = f
        .tree()
        .into_iter()
        .filter(|e| e.contains(".tmp") || e.contains("claude.json."))
        .collect();
    assert!(stray.is_empty(), "temporary files left behind: {stray:?}");
}

#[test]
fn awkward_characters_in_values_survive() {
    let f = machine();
    f.commons_mcp(
        r#"{"mcpServers": {"odd": {"command": "x",
             "env": {"QUOTES": "a\"b", "SLASH": "a\\b", "UNICODE": "héllo→世界"}}}}"#,
    );

    f.run(&["sync"]).assert_clean();

    let env = &f.json(".claude.json")["mcpServers"]["odd"]["env"];
    assert_eq!(env["QUOTES"], "a\"b");
    assert_eq!(env["SLASH"], "a\\b");
    assert_eq!(env["UNICODE"], "héllo→世界");
}

#[test]
fn status_reports_mcp_in_json() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(
        ".claude.json",
        r#"{"mcpServers": {"handmade": {"command": "mine"}}}"#,
    );

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entries = parsed["mcp"].as_array().expect("an mcp array");
    let serena = entries
        .iter()
        .find(|e| e["name"] == "serena")
        .expect("the Commons server");
    assert_eq!(serena["state"], "missing");
    let handmade = entries.iter().find(|e| e["name"] == "handmade").unwrap();
    assert_eq!(handmade["state"], "foreign");
    assert_eq!(handmade["actionable"], false);
}

#[test]
fn a_held_lock_stops_the_write() {
    let f = machine();
    f.commons_mcp(one_server());
    let _held = common::hold_lock(f.path(".local/state/agent-sync/lock"));

    let out = f.run_with_env(&["sync"], &[("AGENT_SYNC_LOCK_TIMEOUT_MS", "150")]);

    out.assert_code(1).assert_stderr_has("another agent-sync");
    assert!(!f.present(".claude.json"));
}

#[test]
fn a_toml_agent_is_merged_without_losing_the_users_settings() {
    // Codex keeps MCP in the same file as its model settings. Writing the
    // standard JSON shape here would corrupt it, so it gets a TOML merge.
    let f = machine();
    f.agent(".codex");
    f.commons_mcp(one_server());
    f.file(".codex/config.toml", "model = \"gpt-5\"\n");

    let out = f.run(&["sync"]);

    out.assert_clean();
    let toml = f.contents(".codex/config.toml");
    assert!(
        toml.contains("model = \"gpt-5\""),
        "the user's own settings survive:\n{toml}"
    );
    assert!(toml.contains("[mcp_servers.serena]"), "got:\n{toml}");
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx",
        "and the JSON agents are synced in the same run"
    );
}

#[test]
fn a_fresh_codex_never_receives_a_json_document() {
    let f = machine();
    f.agent(".codex");
    f.commons_mcp(one_server());

    f.run(&["sync"]).assert_clean();

    let toml = f.contents(".codex/config.toml");
    assert!(
        !toml.trim_start().starts_with('{'),
        "Codex must get TOML, never a JSON document:\n{toml}"
    );
    assert!(toml.contains("[mcp_servers.serena]"), "got:\n{toml}");
    // And it is genuinely parseable as TOML by a second run.
    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");
}

#[test]
fn one_broken_config_does_not_deny_service_to_the_others() {
    let f = machine();
    f.agent(".cursor");
    f.commons_mcp(one_server());
    f.file(".cursor/mcp.json", "not json at all");

    let out = f.run(&["sync"]);

    // The broken file is reported and left alone...
    out.assert_code(1).assert_stderr_has(".cursor/mcp.json");
    assert_eq!(f.contents(".cursor/mcp.json"), "not json at all");
    // ...while every healthy agent is still synced.
    assert_eq!(
        f.json(".claude.json")["mcpServers"]["serena"]["command"],
        "uvx"
    );
}

#[test]
fn a_broken_config_never_blanks_out_the_status_report() {
    let f = machine();
    f.agent(".cursor");
    f.commons_mcp(one_server());
    f.commons_skill("research");
    f.file(".cursor/mcp.json", "not json at all");

    let out = f.run(&["status"]);

    assert!(
        !out.stdout.is_empty(),
        "a read-only report must survive one unreadable third-party file"
    );
    out.assert_stdout_has("research");
}

#[test]
fn a_document_that_is_not_an_object_is_never_replaced() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(".claude.json", "[1, 2, 3]");

    let out = f.run(&["sync"]);

    out.assert_code(1);
    assert_eq!(
        f.contents(".claude.json"),
        "[1, 2, 3]",
        "someone else's data in a shape we do not understand is not ours to replace"
    );
}

#[test]
fn a_non_object_server_map_is_reported_not_overwritten() {
    let f = machine();
    f.commons_mcp(one_server());
    f.file(".claude.json", r#"{"mcpServers": "unexpected", "keep": 1}"#);

    let out = f.run(&["sync"]);

    out.assert_code(1);
    assert_eq!(f.json(".claude.json")["mcpServers"], "unexpected");
}

#[test]
fn secrets_landing_in_a_readable_file_are_called_out() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"api": {"command": "x", "env": {"K": "${env:TOKEN}"}}}}"#);
    f.file(".claude.json", "{}");
    let readable = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(f.path(".claude.json"), readable).unwrap();

    let out = f.run_with_env(&["sync"], &[("TOKEN", CANARY)]);

    out.assert_clean()
        .assert_stderr_has("resolved secrets")
        .assert_no_output_contains(CANARY);
}

#[test]
fn a_dangling_symlink_destination_keeps_its_link() {
    let f = machine();
    f.commons_mcp(one_server());
    let real = f.root().join("dotfiles").join("claude.json");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    // The dotfiles repo is checked out but this file has not been created yet.
    f.symlink(".claude.json", &real.display().to_string());

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".claude.json"), "the link must survive");
    assert!(real.exists(), "and its target should now exist");
}

#[test]
fn the_commons_file_has_one_documented_shape() {
    let f = machine();
    // A bare map is not the standard shape; guessing would turn `$schema` into
    // a server name.
    f.commons_mcp(r#"{"serena": {"command": "uvx"}}"#);

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("mcpServers");
}

#[test]
fn a_server_that_is_not_an_object_is_refused() {
    let f = machine();
    f.commons_mcp(r#"{"mcpServers": {"broken": "should be an object"}}"#);

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("must be an object");
}

#[test]
fn one_change_is_reported_in_the_singular() {
    let f = machine();
    f.commons_mcp(one_server());

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("1 change made.");
}
