//! The seam itself: how the CLI resolves the Commons and home from the
//! environment, and how it separates results from diagnostics.

mod common;

use common::Fixture;
use std::fs;

#[test]
fn target_root_redirects_home_without_touching_the_real_one() {
    let f = Fixture::new();
    f.agent(".claude");

    // Only AGENT_SYNC_TARGET_ROOT is set: the Commons defaults to <home>/.agents.
    let out = f.run_with_vars(
        &["doctor"],
        &[("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string())],
    );

    out.assert_clean()
        .assert_stdout_has(&f.home().join(".agents").display().to_string())
        .assert_stdout_has("claude");
}

#[test]
fn commons_location_is_independent_of_home() {
    let f = Fixture::new();
    f.agent(".claude");
    let elsewhere = f.root().join("commons-elsewhere");
    fs::create_dir_all(elsewhere.join("skills")).unwrap();
    fs::create_dir_all(elsewhere.join("skills").join("moved")).unwrap();

    let out = f.run_with_vars(
        &["doctor"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", elsewhere.display().to_string()),
        ],
    );

    out.assert_clean()
        .assert_stdout_has(&elsewhere.display().to_string())
        .assert_stdout_has("skills        1");
}

#[test]
fn falls_back_to_home_when_target_root_is_unset() {
    let f = Fixture::new();
    f.agent(".codex");

    let out = f.run_with_vars(&["doctor"], &[("HOME", f.home().display().to_string())]);

    out.assert_clean().assert_stdout_has("codex");
}

#[test]
fn target_root_wins_over_home() {
    let f = Fixture::new();
    f.agent(".codex");
    let decoy = f.root().join("decoy-home");
    fs::create_dir_all(decoy.join(".claude")).unwrap();

    let out = f.run_with_vars(
        &["doctor"],
        &[
            ("HOME", decoy.display().to_string()),
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
        ],
    );

    out.assert_clean()
        .assert_stdout_has("codex")
        .assert_stdout_lacks("claude");
}

#[test]
fn without_any_home_it_refuses_to_guess() {
    let f = Fixture::new();

    let out = f.run_with_vars(&["doctor"], &[]);

    out.assert_code(1).assert_stderr_has("home directory");
}

#[test]
fn config_directory_is_never_inside_the_commons() {
    let f = Fixture::new();

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has(&f.home().join(".config/agent-sync").display().to_string())
        .assert_stdout_has(
            &f.home()
                .join(".local/state/agent-sync")
                .display()
                .to_string(),
        );
    assert!(
        !f.commons().join(".config").exists(),
        "tool config must not live in the Commons"
    );
}

#[test]
fn an_absolute_xdg_config_home_wins_over_the_derived_default() {
    let f = Fixture::new();
    let elsewhere = f.home().join("xdg-elsewhere").display().to_string();

    let out = f.run_with_env(&["doctor"], &[("XDG_CONFIG_HOME", &elsewhere)]);

    out.assert_clean()
        .assert_stdout_has(&format!("{elsewhere}/agent-sync"));
}

#[test]
fn a_relative_xdg_config_home_is_ignored() {
    let f = Fixture::new();

    let out = f.run_with_env(&["doctor"], &[("XDG_CONFIG_HOME", "relative/path")]);

    out.assert_clean()
        .assert_stdout_has(&f.home().join(".config/agent-sync").display().to_string());
}

#[test]
fn the_lock_lives_in_the_state_dir_and_the_legacy_dir_stays_absent() {
    let f = Fixture::new();

    f.run(&["sync"]).assert_clean();

    assert!(
        f.present(".local/state/agent-sync/lock"),
        "the lock must land in the XDG state dir"
    );
    assert!(
        !f.present(".agentstow"),
        "nothing may be created at the legacy config dir"
    );
}

#[test]
fn doctor_names_a_leftover_legacy_config_dir_without_inventing_a_config() {
    let f = Fixture::new();
    f.dir(".agentstow");

    let out = f.run(&["doctor"]);

    // v1 kept its lock here as well as its config, so a leftover holding no
    // agentstow.toml is the ordinary case for anyone upgrading. Naming a file
    // that is not there sends them looking for something that never existed.
    out.assert_clean()
        .assert_stderr_has("no longer read")
        .assert_stderr_has("can be deleted")
        .assert_stderr_lacks("move agentstow.toml");
}

#[test]
fn doctor_says_to_move_a_legacy_config_that_is_really_there() {
    let f = Fixture::new();
    f.file(".agentstow/agentstow.toml", "");

    let out = f.run(&["doctor"]);

    // The leftover is named by its old name, and the destination by the new
    // one: both halves have to be right or the instruction cannot be followed.
    out.assert_clean()
        .assert_stderr_has("move agentstow.toml")
        .assert_stderr_has(".config/agent-sync")
        .assert_stderr_has("as agent-sync.toml");
}

#[test]
fn doctor_names_the_xdg_directory_agentstow_left_behind() {
    let f = Fixture::new();
    f.file(".config/agentstow/agentstow.toml", "");

    let out = f.run(&["doctor"]);

    // The rename adds a third leftover layer beside v1's ~/.agentstow. It is
    // never read, so an unnoticed one silently strands the user's settings.
    out.assert_clean()
        .assert_stderr_has(".config/agentstow")
        .assert_stderr_has("move agentstow.toml")
        .assert_stderr_has("as agent-sync.toml");
}

#[test]
fn help_is_a_result_not_a_diagnostic() {
    let f = Fixture::new();

    let out = f.run(&["--help"]);

    out.assert_clean()
        .assert_stderr_empty()
        .assert_stdout_has("agent-sync");
}

#[test]
fn an_unknown_command_is_a_diagnostic() {
    let f = Fixture::new();

    let out = f.run(&["frobnicate"]);

    out.assert_code(1);
    assert!(!out.stderr.is_empty(), "usage errors belong on stderr");
    assert!(out.stdout.is_empty(), "a failed parse produces no results");
}

#[test]
fn the_binary_reports_the_crate_version() {
    // scripts/build-npm.sh reads this version out of Cargo.toml and stamps it
    // onto all five npm packages. If the binary disagreed, a release would ship
    // packages whose name promises one version and whose contents are another.
    let f = Fixture::new();

    let out = f.run(&["--version"]);

    out.assert_clean()
        .assert_stderr_empty()
        .assert_stdout_has(env!("CARGO_PKG_VERSION"));
}
