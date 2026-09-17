//! The glossary, made executable — as far as that is honest.
//!
//! CONTEXT.md defines the project's language and lists retired words under
//! _Avoid_. This test scans every string literal in `src/**/*.rs` — the pool
//! every user-facing message is drawn from — and fails on the avoid-terms that
//! can be checked mechanically without drowning in false positives:
//!
//! * **`store`** (word-bounded, any case) — the pre-v2 name for the Commons.
//!   Ordinary English containing the letters — "restored", "restoring",
//!   "stored" — is not word-bounded and never matches.
//! * **`subagents`** (word-bounded) — the agents family's pre-v2 name. One
//!   occurrence is allowlisted: `Family::PRE_V2_AGENTS_NAME` in
//!   `src/family.rs`, the single literal behind both the config-key alias and
//!   doctor's leftover-directory hint. The count is pinned at exactly one, so
//!   both a new literal and a stale allowlist fail.
//! * The avoid-**phrases** with no legitimate other sense: "canonical source",
//!   "central repo", "agent-sync's directory", "zero-config agent",
//!   "linked skill", "repo-backed".
//!
//! What this test deliberately does NOT enforce, and why: several avoid-words
//! are homonyms with load-bearing legitimate uses, so a blanket scan would
//! flag correct sentences and train people to allowlist. "source" is the worst
//! offender — banned for the central directory, yet **Source** is itself a
//! defined term (the outside home of a Sourced entry) that messages must say.
//! "linked" is the fan-out mechanism's own vocabulary ("1 linked", "will be
//! linked"), so only the phrase "linked skill" is banned. "external",
//! "override" and "conflict" have the same problem — **Conflict** is a defined
//! term with its own glossary entry — as do ordinary verbs like "import"
//! ("import line" is a mechanism), "add", "reset" and "delete". Those words
//! rely on review against CONTEXT.md, not on this test; partial enforcement
//! here must not be mistaken for full.
//!
//! The enforced terms are read back out of CONTEXT.md at runtime: if the
//! glossary ever drops one, this test fails so the rules are re-derived
//! rather than silently enforcing a stale glossary.
//!
//! Scope and mechanics: string literals only — comments, identifiers and doc
//! text are not user-facing output. Extraction is a pragmatic hand-rolled
//! scanner (comments skipped, char literals and raw strings handled, escape
//! sequences read as word boundaries); a `#[cfg(test)]` marker in a source
//! file ends its scan, on the idiomatic assumption that test modules sit at
//! the end of a file. Sources outside `src/` — Cargo.toml's description, the
//! npm launcher's — are also user-facing but are not Rust and are not scanned.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A string literal found in a source file.
struct Literal {
    file: PathBuf,
    line: usize,
    text: String,
}

/// Words banned outright, matched word-bounded and case-insensitively.
const BANNED_WORDS: &[&str] = &["store"];

/// The retired family name: word-bounded, one pinned occurrence permitted.
const RETIRED_FAMILY: &str = "subagents";
const RETIRED_FAMILY_FILE: &str = "family.rs";
const RETIRED_FAMILY_PERMITTED: usize = 1;

/// Exact phrases banned outright (case-insensitive, word-bounded, a trailing
/// plural `s` tolerated).
const BANNED_PHRASES: &[&str] = &[
    "canonical source",
    "central repo",
    "agent-sync's directory",
    "zero-config agent",
    "linked skill",
    "repo-backed",
];

#[test]
fn user_facing_strings_follow_the_glossary() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("src");

    let mut files = Vec::new();
    collect_rust_files(&src, &mut files);
    files.sort();
    assert!(
        files.len() >= 20,
        "expected the src tree, found {} .rs files — extraction is broken",
        files.len()
    );

    let mut literals = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_else(|e| panic!("read {file:?}: {e}"));
        // Test modules are not user-facing; idiomatically they end the file.
        let source = match source.find("#[cfg(test)]") {
            Some(at) => &source[..at],
            None => &source[..],
        };
        extract_string_literals(source, file, &mut literals);
    }
    assert!(
        literals.len() >= 100,
        "expected hundreds of string literals, found {} — extraction is broken",
        literals.len()
    );

    let mut violations = Vec::new();
    let mut retired_family_hits = 0usize;

    for lit in &literals {
        for word in BANNED_WORDS {
            if has_word(&lit.text, word) {
                violations.push(describe(lit, &format!("retired word `{word}`")));
            }
        }

        if has_word(&lit.text, RETIRED_FAMILY) {
            if lit
                .file
                .file_name()
                .is_some_and(|n| n == RETIRED_FAMILY_FILE)
            {
                retired_family_hits += 1;
            } else {
                violations.push(describe(lit, "retired family name `subagents`"));
            }
        }

        for phrase in BANNED_PHRASES {
            if has_phrase(&lit.text, phrase) {
                violations.push(describe(lit, &format!("avoid-phrase \"{phrase}\"")));
            }
        }
    }

    assert_eq!(
        retired_family_hits, RETIRED_FAMILY_PERMITTED,
        "src/{RETIRED_FAMILY_FILE} must hold exactly {RETIRED_FAMILY_PERMITTED} \
         `subagents` literal (Family::PRE_V2_AGENTS_NAME); found {retired_family_hits}"
    );

    assert!(
        violations.is_empty(),
        "user-facing strings drifted from CONTEXT.md's language:\n{}",
        violations.join("\n")
    );

    glossary_still_defines_the_enforced_terms(&manifest);
}

/// The freshness check: the rules above are derived from CONTEXT.md, so the
/// glossary must still carry them. A missing term here means the glossary
/// changed and this test needs re-deriving, not that enforcement may lapse.
fn glossary_still_defines_the_enforced_terms(manifest: &Path) {
    let context = fs::read_to_string(manifest.join("CONTEXT.md"))
        .expect("CONTEXT.md must sit next to Cargo.toml");

    let avoid: BTreeSet<String> = context
        .lines()
        .filter_map(|line| line.trim().strip_prefix("_Avoid_:"))
        .flat_map(|rest| rest.split(','))
        .map(|term| term.trim().to_string())
        .collect();

    // "linked skill" is enforced as a phrase because bare "linked" — the term
    // the glossary actually retires for Sourced entries — is the fan-out
    // vocabulary and cannot be banned mechanically.
    for term in [
        "store",
        "canonical source",
        "central repo",
        "agent-sync's directory",
        "zero-config agent",
        "linked",
        "repo-backed",
    ] {
        assert!(
            avoid.contains(term),
            "CONTEXT.md no longer lists \"{term}\" under _Avoid_ — \
             re-derive this test's rules from the current glossary"
        );
    }

    assert!(
        !context.to_ascii_lowercase().contains(RETIRED_FAMILY),
        "CONTEXT.md now mentions `subagents` — re-derive this test's rules"
    );
    assert!(
        context.contains("commands, agents"),
        "CONTEXT.md no longer names the `agents` family — re-derive this test's rules"
    );
}

fn describe(lit: &Literal, why: &str) -> String {
    format!(
        "  {}:{}: {} in {:?}",
        lit.file.display(),
        lit.line,
        why,
        lit.text
    )
}

fn collect_rust_files(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, acc);
        } else if path.extension().is_some_and(|e| e == "rs") {
            acc.push(path);
        }
    }
}

/// Whether `word` appears in `text` as a whole word (case-insensitive):
/// tokenized on non-alphanumeric boundaries, so "restored" never yields
/// "store" and "SubagentStop" never yields "subagents".
fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token.eq_ignore_ascii_case(word))
}

/// Whether `phrase` appears in `text` case-insensitively, bounded by
/// non-alphanumerics on both sides, with a trailing plural `s` tolerated —
/// so "symlinked skills" does not match "linked skill", but "linked skills"
/// standing alone would.
fn has_phrase(text: &str, phrase: &str) -> bool {
    let hay = text.to_ascii_lowercase();
    let bytes = hay.as_bytes();
    for (at, _) in hay.match_indices(phrase) {
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let end = at + phrase.len();
        let after_ok = match bytes.get(end) {
            None => true,
            Some(b) if !b.is_ascii_alphanumeric() => true,
            Some(b's') => !bytes
                .get(end + 1)
                .is_some_and(|b| b.is_ascii_alphanumeric()),
            Some(_) => false,
        };
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Pull every string literal out of one file's source text.
///
/// A pragmatic scanner, not a parser: line and block comments are skipped,
/// char literals (including `'"'`) and lifetimes are stepped over, raw strings
/// (`r"…"`, `r#"…"#`) are read to their closing delimiter, and every escape
/// sequence inside a literal is recorded as a single space so `"a\nstore"`
/// still word-bounds. Good enough to be exact on this codebase; the sanity
/// asserts in the test catch it silently breaking.
fn extract_string_literals(source: &str, file: &Path, acc: &mut Vec<Literal>) {
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut prev_ident = false; // was the previous char part of an identifier?

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
                prev_ident = false;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                prev_ident = false;
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                prev_ident = false;
            }
            '\'' => {
                // Char literal or lifetime. `'\x'` and `'"'` are literals; a
                // bare tick followed by an identifier is a lifetime.
                if chars.get(i + 1) == Some(&'\\') {
                    i += 2; // consume the tick and the backslash
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                } else if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
                prev_ident = false;
            }
            'r' if !prev_ident => {
                // Possibly a raw string: r"…" or r#"…"#.
                let mut j = i + 1;
                let mut hashes = 0usize;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'"') {
                    j += 1;
                    let start_line = line;
                    let mut text = String::new();
                    'raw: while j < chars.len() {
                        if chars[j] == '"' {
                            let mut k = 0usize;
                            while k < hashes && chars.get(j + 1 + k) == Some(&'#') {
                                k += 1;
                            }
                            if k == hashes {
                                j += 1 + hashes;
                                break 'raw;
                            }
                        }
                        if chars[j] == '\n' {
                            line += 1;
                        }
                        text.push(chars[j]);
                        j += 1;
                    }
                    acc.push(Literal {
                        file: file.to_path_buf(),
                        line: start_line,
                        text,
                    });
                    i = j;
                    prev_ident = false;
                } else {
                    i += 1;
                    prev_ident = true;
                }
            }
            '"' => {
                let start_line = line;
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        // An escape is a boundary, never letters that could
                        // glue two words into one.
                        text.push(' ');
                        if chars.get(i + 1) == Some(&'\n') {
                            line += 1;
                        }
                        i += 2;
                        continue;
                    }
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    text.push(chars[i]);
                    i += 1;
                }
                i += 1; // closing quote
                acc.push(Literal {
                    file: file.to_path_buf(),
                    line: start_line,
                    text,
                });
                prev_ident = false;
            }
            _ => {
                prev_ident = c.is_ascii_alphanumeric() || c == '_';
                i += 1;
            }
        }
    }
}
