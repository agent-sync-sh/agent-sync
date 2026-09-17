//! `$XDG_CONFIG_HOME/agent-sync/agent-sync.toml` — the only place agent-sync's own settings live.

mod common;

use common::Fixture;

fn config(f: &Fixture, body: &str) {
    f.file(".config/agent-sync/agent-sync.toml", body);
}

#[test]
fn a_disabled_target_disappears_everywhere() {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".cursor");
    f.commons_skill("research");
    config(&f, "[targets]\ncursor = false\n");

    f.run(&["sync"]).assert_clean();
    assert!(f.is_symlink(".claude/skills/research"));
    assert!(
        !f.present(".cursor/skills/research"),
        "a disabled target must not be synced"
    );

    f.run(&["status"]).assert_stdout_lacks("cursor");
    f.run(&["doctor"]).assert_stdout_lacks("cursor");
}

#[test]
fn a_custom_target_is_treated_like_any_other() {
    let f = Fixture::new();
    f.agent(".myagent");
    f.commons_skill("research");
    config(
        &f,
        "[custom.myagent]\nroot = \".myagent\"\nskills = \".myagent/skills\"\n",
    );

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".myagent/skills/research"));
    assert_eq!(
        f.link_text(".myagent/skills/research"),
        "../../.agents/skills/research"
    );
    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_has("myagent");
}

#[test]
fn the_pre_v2_subagents_config_key_still_works() {
    let f = Fixture::new();
    f.agent(".myagent");
    f.commons_file("agents/reviewer.md", "review\n");
    config(
        &f,
        "[custom.myagent]\nroot = \".myagent\"\nsubagents = \".myagent/agents\"\n",
    );

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".myagent/agents/reviewer.md"));
}

#[test]
fn a_custom_target_that_is_not_installed_stays_inert() {
    let f = Fixture::new();
    f.commons_skill("research");
    config(
        &f,
        "[custom.myagent]\nroot = \".myagent\"\nskills = \".myagent/skills\"\n",
    );

    f.run(&["sync"]).assert_clean();

    assert!(
        !f.exists(".myagent"),
        "config must not conjure an agent root into existence"
    );
}

#[test]
fn no_config_file_means_defaults() {
    let f = Fixture::new();
    f.agent(".claude");
    f.commons_skill("research");

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".claude/skills/research"));
}

#[test]
fn an_empty_config_file_means_defaults() {
    let f = Fixture::new();
    f.agent(".claude");
    f.commons_skill("research");
    config(&f, "");

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".claude/skills/research"));
}

#[test]
fn malformed_toml_is_a_clear_error() {
    let f = Fixture::new();
    f.agent(".claude");
    config(&f, "[targets\nbroken =\n");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("agent-sync.toml");
}

#[test]
fn a_non_boolean_target_switch_is_rejected() {
    let f = Fixture::new();
    f.agent(".claude");
    config(&f, "[targets]\nclaude = \"yes\"\n");

    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("true or false");
}

#[test]
fn a_custom_target_without_a_root_is_rejected() {
    let f = Fixture::new();
    config(&f, "[custom.myagent]\nskills = \".myagent/skills\"\n");

    f.run(&["sync"]).assert_code(1).assert_stderr_has("root");
}

#[test]
fn configuration_never_lands_in_the_commons() {
    let f = Fixture::new();
    f.agent(".claude");
    config(&f, "[targets]\ncursor = false\n");

    f.run(&["sync"]).assert_clean();

    assert!(
        !f.commons().join("agent-sync.toml").exists(),
        "the Commons holds ecosystem content only"
    );
}

#[test]
fn a_disabled_target_is_not_counted_as_missing() {
    let f = Fixture::new();
    f.agent(".claude");
    config(&f, "[targets]\ncursor = false\n");

    let out = f.run(&["doctor"]);

    let total = agent_sync::registry::AGENTS.len() - 1;
    out.assert_stdout_has(&format!("1 of {total}"));
}
