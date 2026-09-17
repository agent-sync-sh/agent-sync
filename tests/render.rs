//! Rendered whole files — the third ownership identity. A file agent-sync
//! generated carries a Marker; one without a Marker was written by somebody
//! else and is never touched.

mod common;

use common::Fixture;

fn machine() -> Fixture {
    let f = Fixture::new();
    f.agent(".gemini");
    f.agent(".claude");
    f
}

fn a_command(f: &Fixture) {
    f.commons_file(
        "commands/ship.md",
        "---\nname: ship\ndescription: Ship the current branch\n---\n\nDo the thing.\n",
    );
}

#[test]
fn a_commons_command_becomes_a_gemini_toml_file() {
    let f = machine();
    a_command(&f);

    f.run(&["sync"]).assert_clean();

    let toml = f.contents(".gemini/commands/ship.toml");
    assert!(
        toml.contains("description = \"Ship the current branch\""),
        "got:\n{toml}"
    );
    assert!(toml.contains("Do the thing."), "got:\n{toml}");
    assert!(
        toml.lines().next().unwrap_or("").contains("agent-sync"),
        "the Marker must be the first thing in the file:\n{toml}"
    );
}

#[test]
fn the_rendered_file_is_valid_toml_gemini_can_read() {
    let f = machine();
    a_command(&f);

    f.run(&["sync"]).assert_clean();

    let parsed: toml::Value = toml::from_str(&f.contents(".gemini/commands/ship.toml"))
        .expect("Gemini must be able to parse what we wrote");
    assert_eq!(
        parsed["description"].as_str(),
        Some("Ship the current branch")
    );
    assert!(parsed["prompt"].as_str().unwrap().contains("Do the thing."));
}

#[test]
fn markdown_agents_still_get_a_link_not_a_render() {
    let f = machine();
    a_command(&f);

    f.run(&["sync"]).assert_clean();

    assert!(f.is_symlink(".claude/commands/ship.md"));
    assert!(!f.present(".claude/commands/ship.toml"));
}

#[test]
fn editing_the_commons_re_renders_the_file() {
    let f = machine();
    a_command(&f);
    f.run(&["sync"]).assert_clean();

    f.commons_file(
        "commands/ship.md",
        "---\nname: ship\ndescription: Ship it faster\n---\n\nDo it differently.\n",
    );
    f.run(&["sync"]).assert_clean();

    let toml = f.contents(".gemini/commands/ship.toml");
    assert!(toml.contains("Ship it faster"), "got:\n{toml}");
    assert!(toml.contains("Do it differently."), "got:\n{toml}");
    assert!(!toml.contains("Do the thing."), "got:\n{toml}");
}

#[test]
fn deleting_the_commons_command_prunes_the_rendered_file() {
    let f = machine();
    a_command(&f);
    f.run(&["sync"]).assert_clean();
    std::fs::remove_file(f.commons().join("commands/ship.md")).unwrap();

    f.run(&["sync"]).assert_clean();

    assert!(
        !f.present(".gemini/commands/ship.toml"),
        "the Marker proves we wrote it, so removing it is safe"
    );
}

#[test]
fn a_hand_written_file_at_the_same_path_is_never_touched() {
    let f = machine();
    a_command(&f);
    let theirs = "description = \"mine, by hand\"\nprompt = \"\"\"\nleave me alone\n\"\"\"\n";
    f.file(".gemini/commands/ship.toml", theirs);

    let out = f.run(&["sync"]);

    out.assert_clean();
    assert_eq!(
        f.contents(".gemini/commands/ship.toml"),
        theirs,
        "no Marker means it was not ours to write"
    );
    out.assert_stdout_has("foreign");
}

#[test]
fn a_hand_written_file_is_never_pruned_either() {
    let f = machine();
    // Nothing in the Commons claims this name at all.
    let theirs = "description = \"theirs\"\nprompt = \"\"\"\nx\n\"\"\"\n";
    f.file(".gemini/commands/plannotator-review.toml", theirs);
    f.commons_file(
        "commands/other.md",
        "---\ndescription: Other\n---\n\nBody.\n",
    );

    f.run(&["sync"]).assert_clean();

    assert_eq!(
        f.contents(".gemini/commands/plannotator-review.toml"),
        theirs
    );
}

#[test]
fn a_marked_file_edited_by_hand_is_restored_and_the_change_is_shown() {
    let f = machine();
    a_command(&f);
    f.run(&["sync"]).assert_clean();

    let tampered = format!(
        "{}\ndescription = \"tampered\"\nprompt = \"\"\"\nnope\n\"\"\"\n",
        f.contents(".gemini/commands/ship.toml")
            .lines()
            .next()
            .unwrap()
    );
    f.file(".gemini/commands/ship.toml", &tampered);

    let out = f.run(&["sync"]);

    out.assert_clean().assert_stdout_has("ship");
    assert!(
        f.contents(".gemini/commands/ship.toml")
            .contains("Ship the current branch")
    );
}

#[test]
fn a_second_sync_changes_nothing() {
    let f = machine();
    a_command(&f);
    f.run(&["sync"]).assert_clean();
    let before = f.contents(".gemini/commands/ship.toml");

    f.run(&["sync"])
        .assert_clean()
        .assert_stdout_has("up to date");

    assert_eq!(before, f.contents(".gemini/commands/ship.toml"));
}

#[test]
fn a_command_with_no_frontmatter_still_renders() {
    let f = machine();
    f.commons_file("commands/bare.md", "Just a body, no frontmatter.\n");

    f.run(&["sync"]).assert_clean();

    let parsed: toml::Value = toml::from_str(&f.contents(".gemini/commands/bare.toml")).unwrap();
    assert!(parsed["prompt"].as_str().unwrap().contains("Just a body"));
}

#[test]
fn awkward_characters_in_the_body_survive() {
    let f = machine();
    f.commons_file(
        "commands/odd.md",
        "---\ndescription: Has \"quotes\" and \\backslashes\n---\n\nA \"\"\"triple quote\"\"\" and a \\ and \u{e9}.\n",
    );

    f.run(&["sync"]).assert_clean();

    let parsed: toml::Value = toml::from_str(&f.contents(".gemini/commands/odd.toml"))
        .expect("escaping must produce valid TOML");
    let prompt = parsed["prompt"].as_str().unwrap();
    assert!(prompt.contains("triple quote"), "got: {prompt}");
    assert!(prompt.contains('\u{e9}'), "got: {prompt}");
}

#[test]
fn status_reports_the_rendered_family() {
    let f = machine();
    a_command(&f);

    let out = f.run(&["status", "--json"]);
    out.assert_code(2);

    let parsed: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let rendered = parsed["rendered"].as_array().expect("a rendered array");
    assert!(
        rendered
            .iter()
            .any(|e| e["agent"] == "gemini" && e["state"] == "missing")
    );
}

#[test]
fn dry_run_writes_nothing() {
    let f = machine();
    a_command(&f);
    let before = f.tree();

    f.run(&["sync", "--dry-run"])
        .assert_clean()
        .assert_stdout_has("ship");

    assert_eq!(before, f.tree());
}

#[test]
fn agents_with_no_render_surface_get_nothing() {
    let f = Fixture::new();
    f.agent(".codex");
    a_command(&f);

    f.run(&["sync"]).assert_clean();

    assert!(!f.exists(".codex/commands"));
    // Codex takes markdown commands by link instead.
    assert!(f.is_symlink(".codex/prompts/ship.md"));
}
