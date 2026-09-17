//! `doctor` — machine readiness: which agents are installed, is the Commons
//! usable, and is anything in it going to be silently skipped.

mod common;

use common::Fixture;

#[test]
fn reports_only_agents_whose_config_root_exists() {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".codex");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has("claude")
        .assert_stdout_has("codex")
        .assert_stdout_lacks("windsurf")
        .assert_stdout_lacks("cline");
}

#[test]
fn every_registry_agent_is_detected_by_creating_its_root() {
    // The registry is data: a row's `root` is the whole detection rule. If this
    // holds for every row, adding an agent really is a data-only change.
    for agent in agent_sync::registry::AGENTS {
        let f = Fixture::new();
        f.agent(agent.root);

        f.run(&["doctor"])
            .assert_clean()
            .assert_stdout_has(agent.name);
    }
}

#[test]
fn reports_the_capabilities_of_a_detected_agent() {
    let f = Fixture::new();
    f.agent(".claude");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_has("import-line")
        .assert_stdout_has(".claude/skills");
}

#[test]
fn names_the_legacy_dir_a_flipped_native_agent_still_reads() {
    // Codex and cursor read ~/.agents/skills natively (flipped 2026-08-16)
    // but still read their old fan-out dirs, which sync prunes; gemini is
    // Native with nothing to clean.
    let f = Fixture::new();
    f.agent(".codex");
    f.agent(".cursor");
    f.agent(".gemini");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_has("skills native (reads the Commons; cleans .codex/skills)")
        .assert_stdout_has("skills native (reads the Commons; cleans .cursor/skills)")
        .assert_stdout_has("skills native (reads the Commons)\n");
}

#[test]
fn creates_nothing() {
    let f = Fixture::new();
    f.agent(".claude");
    f.agent(".gemini");
    f.commons_skill("research");
    let before = f.tree();

    f.run(&["doctor"]).assert_clean();

    assert_eq!(before, f.tree(), "doctor must be strictly read-only");
}

#[test]
fn does_not_create_roots_for_agents_that_are_not_installed() {
    let f = Fixture::new();

    f.run(&["doctor"]).assert_clean();

    for agent in agent_sync::registry::AGENTS {
        assert!(
            !f.path(agent.root).exists(),
            "doctor created a config root for {}",
            agent.name
        );
    }
}

#[test]
fn warns_about_commons_entries_that_agents_would_never_see() {
    let f = Fixture::new();
    f.commons_skill("research");
    // Dot-prefixed: invisible to the agents' own scanners.
    f.commons_file("skills/.hidden/SKILL.md", "hidden\n");
    // A loose file where a skill directory belongs.
    f.commons_file("skills/README.md", "not a skill\n");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has("skills        1")
        .assert_stderr_has(".hidden")
        .assert_stderr_has("README.md");
}

#[test]
fn warns_about_a_dangling_symlink_in_the_commons() {
    let f = Fixture::new();
    f.commons_symlink("skills/gone", "../../nowhere");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has("skills        0")
        .assert_stderr_has("gone");
}

#[test]
fn warns_about_a_non_empty_pre_v2_subagents_dir() {
    let f = Fixture::new();
    f.commons_file("subagents/reviewer.md", "review\n");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stderr_has("no longer a family")
        .assert_stderr_has("agents/");
}

#[test]
fn warnings_do_not_make_the_run_fail() {
    let f = Fixture::new();
    f.commons_file("skills/README.md", "not a skill\n");

    // A warning is advisory: it is reported, but the machine is still usable.
    f.run(&["doctor"]).assert_code(0);
}

#[test]
fn missing_commons_is_a_problem_that_names_the_fix() {
    let f = Fixture::bare();

    f.run(&["doctor"])
        .assert_code(1)
        .assert_stderr_has("agent-sync init");
}

#[test]
fn a_clean_machine_reports_nothing_on_stderr() {
    let f = Fixture::new();
    f.agent(".claude");
    f.commons_skill("research");

    let out = f.run(&["doctor"]);

    out.assert_clean().assert_stderr_empty();
    assert!(!out.stdout.is_empty(), "results belong on stdout");
}

#[test]
fn counts_the_agents_it_knows_about_but_did_not_find() {
    let f = Fixture::new();
    f.agent(".claude");

    let total = agent_sync::registry::AGENTS.len();

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_has(&format!("1 of {total}"))
        .assert_stdout_has(&format!("{} known agents are not installed", total - 1));
}

#[test]
fn init_creates_the_commons_and_nothing_else() {
    let f = Fixture::bare();

    f.run(&["init"]).assert_clean().assert_stdout_has("Created");

    assert!(f.commons().join("skills").is_dir());
    assert!(f.commons().join("commands").is_dir());
    assert!(f.commons().join("agents").is_dir());
    assert!(
        !f.commons().join("AGENTS.md").exists(),
        "an empty AGENTS.md would fan out as empty instructions"
    );
    for agent in agent_sync::registry::AGENTS {
        assert!(
            !f.path(agent.root).exists(),
            "init must not create agent roots"
        );
    }
}

#[test]
fn the_advice_to_run_init_actually_works() {
    let f = Fixture::bare();
    f.agent(".claude");
    f.run(&["sync"])
        .assert_code(1)
        .assert_stderr_has("agent-sync init");

    f.run(&["init"]).assert_clean();

    f.run(&["sync"]).assert_clean();
}

#[test]
fn init_is_safe_to_run_twice() {
    let f = Fixture::new();
    f.commons_skill("research");

    f.run(&["init"])
        .assert_clean()
        .assert_stdout_has("already present");

    assert!(f.commons().join("skills/research/SKILL.md").exists());
}

// --- The Sourced view (ADR-0006) --------------------------------------------

#[test]
fn lists_sourced_entries_with_their_sources_and_count() {
    let f = Fixture::new();
    f.file("repo/skills/research/SKILL.md", "kept in the repo\n");
    let source = f.path("repo/skills/research");
    f.commons_symlink("skills/research", &source.display().to_string());
    f.commons_skill("plain");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has("skills        2 (1 sourced)")
        .assert_stdout_has("Sourced entries:")
        .assert_stdout_has(&format!("  skills/research ← {}", source.display()))
        .assert_stdout_lacks("source missing");
}

#[test]
fn a_missing_source_is_still_listed_and_marked() {
    // The scan skips a dangling link, but the Sourced view answers the
    // machine-bootstrap question "what must I clone?" — so it must name
    // exactly the entries whose repo is not here yet.
    let f = Fixture::new();
    // Built from the fixture rather than spelled as a literal: an absolute
    // path is native, and Windows resolves a rooted one against the current
    // drive as the link is created.
    let source = f.path("nowhere/skills/research");
    f.commons_symlink("skills/research", &source.display().to_string());

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has("skills        0 (1 sourced)")
        .assert_stdout_has(&format!(
            "  skills/research ← {} (source missing)",
            source.display()
        ));
}

#[test]
fn the_sourced_suffix_and_section_vanish_when_there_are_none() {
    let f = Fixture::new();
    f.commons_skill("research");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_lacks("sourced")
        .assert_stdout_lacks("Sourced entries");
}

// --- The Commons as a shared commons (ADR-0004) -----------------------------

#[test]
fn names_commons_entries_that_are_not_ours() {
    // `~/.agents/` is a commons: opencode, oh-my-pi and hermes read it directly
    // and the `skills` CLI keeps its lock file there. A neighbour's entry is
    // named so the sharing is visible — never called an issue, never counted.
    let f = Fixture::new();
    f.commons_file(".skill-lock.json", "{}");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_has("Other tools in the Commons")
        .assert_stdout_has(".skill-lock.json")
        .assert_no_output_contains("issue");
}

#[test]
fn says_nothing_about_other_tools_when_there_are_none() {
    let f = Fixture::new();
    f.commons_skill("research");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_lacks("Other tools in the Commons");
}

#[test]
fn protocol_surfaces_are_attributed_not_anonymous() {
    // `.agents` Protocol paths are a named kind of Co-tenant: listed under
    // their own heading, never counted as issues, never touched.
    let f = Fixture::new();
    f.commons_file("tasks/daily/task.md", "kind: task\n");
    f.commons_file("models.json", "{}");
    f.commons_file(".skill-lock.json", "{}");

    let out = f.run(&["doctor"]);

    out.assert_clean()
        .assert_stdout_has(".agents Protocol surfaces:")
        .assert_stdout_has("  models.json")
        .assert_stdout_has("  tasks")
        .assert_stdout_has("Other tools in the Commons")
        .assert_stdout_has(".skill-lock.json")
        .assert_no_output_contains("issue");
}

#[test]
fn absent_protocol_surfaces_are_not_mentioned() {
    let f = Fixture::new();
    f.commons_skill("research");
    let before = f.tree();

    f.run(&["doctor"])
        .assert_clean()
        .assert_stdout_lacks("Protocol surfaces");
    assert_eq!(before, f.tree(), "doctor must not create protocol surfaces");
}

#[test]
fn a_relocated_commons_warns_that_native_agents_will_not_see_it() {
    // AGENT_SYNC_HOME moves agent-sync's Commons but cannot move the path a Native
    // agent hardcodes, so the two diverge in silence unless doctor says so.
    let f = Fixture::new();
    f.agent(".config/opencode");
    let elsewhere = f.root().join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("skills")).expect("create alternate Commons");

    f.run_with_vars(
        &["doctor"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", elsewhere.display().to_string()),
        ],
    )
    .assert_clean()
    .assert_stderr_has("opencode")
    .assert_stderr_has("will not see it");
}

#[test]
fn a_relocated_commons_is_silent_when_no_native_agent_is_installed() {
    let f = Fixture::new();
    f.agent(".claude");
    let elsewhere = f.root().join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("skills")).expect("create alternate Commons");

    f.run_with_vars(
        &["doctor"],
        &[
            ("AGENT_SYNC_TARGET_ROOT", f.home().display().to_string()),
            ("AGENT_SYNC_HOME", elsewhere.display().to_string()),
        ],
    )
    .assert_clean()
    .assert_stderr_lacks("will not see it");
}

#[test]
fn the_canonical_commons_never_warns_about_native_agents() {
    let f = Fixture::new();
    f.agent(".config/opencode");

    f.run(&["doctor"])
        .assert_clean()
        .assert_stderr_lacks("will not see it");
}
