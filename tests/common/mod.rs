//! The single testing seam.
//!
//! Every test builds a throwaway tree, points `AGENT_SYNC_TARGET_ROOT` (home) and
//! `AGENT_SYNC_HOME` (Commons) at it, and drives the CLI through `agent_sync::run`.
//! Nothing here touches the real home directory, and no test reaches inside the
//! implementation — assertions are on the resulting filesystem, stdout, stderr
//! and exit code only.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A path rendered with `/` separators, whatever the platform spells it with.
///
/// The comparison partner of [`Fixture::link_text`]: both sides of an assertion
/// about link shape have to be normalised the same way, or they disagree on
/// Windows for a reason that has nothing to do with what is being tested.
pub fn slashed(p: &Path) -> String {
    let text = p.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// Join a `/`-separated relative path onto `base`, one component at a time.
///
/// Every `rel` in this harness is written with forward slashes for
/// readability. `Path::join` keeps them verbatim, which on Windows yields a
/// mixed-separator path like `C:\home\repo/skills/research` — fine to open,
/// but it never string-compares equal to what the code under test emits, since
/// that has been through `normalize` and is all backslashes.
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        out.push(part);
    }
    out
}

/// Create a symlink at `at` whose text is verbatim `target`.
///
/// Mirrors `agent_sync::link::create_symlink` rather than calling it: several
/// tests deliberately build links the production code would never make, and a
/// harness that routed through the code under test could not catch it making
/// the wrong kind. The flavour rule is the same one — Windows links are typed,
/// the flavour comes from what the text resolves to now, and a dangling target
/// falls back to a file link — and the resolution is shared, because
/// reimplementing lexical path resolution here would be its own bug farm.
#[cfg(unix)]
fn make_symlink(target: &str, at: &Path) {
    std::os::unix::fs::symlink(target, at)
        .unwrap_or_else(|e| panic!("create symlink {} -> {target}: {e}", at.display()));
}

#[cfg(windows)]
fn make_symlink(target: &str, at: &Path) {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let make = if agent_sync::link::resolve_link(at, Path::new(target)).is_dir() {
        symlink_dir
    } else {
        symlink_file
    };
    make(target, at).unwrap_or_else(|e| {
        panic!(
            "create symlink {} -> {target}: {e}\n\
             (Windows needs Developer Mode or an elevated shell for this)",
            at.display()
        )
    });
}

/// What one CLI invocation produced.
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    /// Assert a clean exit, showing stderr when it isn't.
    pub fn assert_clean(&self) -> &Self {
        assert_eq!(
            self.code, 0,
            "expected exit 0\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }

    pub fn assert_code(&self, want: i32) -> &Self {
        assert_eq!(
            self.code, want,
            "expected exit {want}\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }

    /// Output with path separators normalised to `/`.
    ///
    /// The CLI prints native paths, so on Windows it reports
    /// `removed .claude\skills\research` where every test asserts the `/`
    /// form. The assertions are about *which entry* was reported, not how the
    /// platform spells a separator, so both sides are normalised before the
    /// comparison rather than every needle being written twice.
    fn shaped(text: &str) -> String {
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text.to_string()
        }
    }

    pub fn assert_stdout_has(&self, needle: &str) -> &Self {
        assert!(
            Self::shaped(&self.stdout).contains(&Self::shaped(needle)),
            "expected stdout to contain {needle:?}\nstdout:\n{}",
            self.stdout
        );
        self
    }

    pub fn assert_stdout_lacks(&self, needle: &str) -> &Self {
        assert!(
            !Self::shaped(&self.stdout).contains(&Self::shaped(needle)),
            "expected stdout NOT to contain {needle:?}\nstdout:\n{}",
            self.stdout
        );
        self
    }

    pub fn assert_stderr_has(&self, needle: &str) -> &Self {
        assert!(
            Self::shaped(&self.stderr).contains(&Self::shaped(needle)),
            "expected stderr to contain {needle:?}\nstderr:\n{}",
            self.stderr
        );
        self
    }

    pub fn assert_stderr_lacks(&self, needle: &str) -> &Self {
        assert!(
            !Self::shaped(&self.stderr).contains(&Self::shaped(needle)),
            "expected stderr NOT to contain {needle:?}\nstderr:\n{}",
            self.stderr
        );
        self
    }

    /// Neither stream may contain this text — used for secret canaries.
    pub fn assert_no_output_contains(&self, needle: &str) -> &Self {
        self.assert_stdout_lacks(needle).assert_stderr_lacks(needle)
    }

    pub fn assert_stderr_empty(&self) -> &Self {
        assert!(
            self.stderr.is_empty(),
            "expected empty stderr, got:\n{}",
            self.stderr
        );
        self
    }
}

/// A throwaway machine: a home directory, and a Commons beside it.
pub struct Fixture {
    _dir: TempDir,
    home: PathBuf,
    commons: PathBuf,
}

impl Fixture {
    /// A machine with no Commons and no agents installed.
    pub fn bare() -> Self {
        let dir = tempfile::tempdir().expect("create tempdir");
        let home = dir.path().join("home");
        fs::create_dir_all(&home).expect("create home");
        let commons = home.join(".agents");
        Self {
            _dir: dir,
            home,
            commons,
        }
    }

    /// A machine with an initialised, empty Commons.
    pub fn new() -> Self {
        let f = Self::bare();
        fs::create_dir_all(f.commons.join("skills")).expect("create Commons");
        f
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn commons(&self) -> &Path {
        &self.commons
    }

    /// The temporary directory containing the home — somewhere to put a second
    /// tree when a test needs one (an alternate Commons, a decoy home).
    pub fn root(&self) -> &Path {
        self._dir.path()
    }

    /// Home-relative path.
    pub fn path(&self, rel: &str) -> PathBuf {
        join_rel(&self.home, rel)
    }

    /// Install an agent by creating its config root.
    pub fn agent(&self, root_rel: &str) -> &Self {
        fs::create_dir_all(self.home.join(root_rel)).expect("create agent root");
        self
    }

    /// Create a home-relative directory.
    pub fn dir(&self, rel: &str) -> &Self {
        fs::create_dir_all(join_rel(&self.home, rel)).expect("create dir");
        self
    }

    /// Write a home-relative file, creating parents.
    pub fn file(&self, rel: &str, body: &str) -> &Self {
        let p = join_rel(&self.home, rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        fs::write(&p, body).expect("write file");
        self
    }

    /// Add a skill to the Commons.
    pub fn commons_skill(&self, name: &str) -> &Self {
        let dir = self.commons.join("skills").join(name);
        fs::create_dir_all(&dir).expect("create Commons skill");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\n---\n\nbody of {name}\n"),
        )
        .expect("write SKILL.md");
        self
    }

    /// Create a Commons-relative symlink pointing at `target` (verbatim, so a
    /// test can create a deliberately dangling one).
    pub fn commons_symlink(&self, rel: &str, target: &str) -> &Self {
        let p = join_rel(&self.commons, rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        make_symlink(target, &p);
        self
    }

    /// Write a Commons-relative file, creating parents.
    pub fn commons_file(&self, rel: &str, body: &str) -> &Self {
        let p = join_rel(&self.commons, rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        fs::write(&p, body).expect("write Commons file");
        self
    }

    /// Run the CLI with both overrides pointed at this fixture.
    pub fn run(&self, args: &[&str]) -> Outcome {
        self.run_with_vars(
            args,
            &[
                ("AGENT_SYNC_TARGET_ROOT", self.home.display().to_string()),
                ("AGENT_SYNC_HOME", self.commons.display().to_string()),
            ],
        )
    }

    /// Run the CLI with an explicit environment — for testing resolution itself.
    pub fn run_with_vars(&self, args: &[&str], vars: &[(&str, String)]) -> Outcome {
        let mut argv: Vec<String> = vec!["agent-sync".to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();

        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = agent_sync::run(&argv, &vars, &mut out, &mut err);

        Outcome {
            code,
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
        }
    }

    /// The verbatim text of a home-relative symlink.
    /// Link text with separators normalised to `/`.
    ///
    /// Tests assert on the *shape* of a link — `../../.agents/skills/research`
    /// — not on which separator the platform spells it with. Windows stores
    /// and returns backslashes, so without this every such assertion would
    /// need a second, identical-looking literal.
    pub fn link_text(&self, rel: &str) -> String {
        let text = fs::read_link(join_rel(&self.home, rel))
            .unwrap_or_else(|e| panic!("{rel} is not a symlink: {e}"))
            .display()
            .to_string();
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text
        }
    }

    /// Whether a home-relative path is a symlink (without following it).
    pub fn is_symlink(&self, rel: &str) -> bool {
        fs::symlink_metadata(join_rel(&self.home, rel))
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    /// Whether a home-relative path exists as a real directory (not a link).
    pub fn is_real_dir(&self, rel: &str) -> bool {
        fs::symlink_metadata(join_rel(&self.home, rel))
            .map(|m| m.is_dir())
            .unwrap_or(false)
    }

    /// Whether a home-relative path exists at all (following links).
    pub fn exists(&self, rel: &str) -> bool {
        join_rel(&self.home, rel).exists()
    }

    /// Whether a home-relative path exists as a link or file, even if broken.
    pub fn present(&self, rel: &str) -> bool {
        fs::symlink_metadata(join_rel(&self.home, rel)).is_ok()
    }

    /// Where a home-relative symlink actually lands, following it.
    pub fn resolves_to(&self, rel: &str) -> PathBuf {
        fs::canonicalize(join_rel(&self.home, rel))
            .unwrap_or_else(|e| panic!("{rel} does not resolve: {e}"))
    }

    /// Create a home-relative symlink with verbatim link text.
    pub fn symlink(&self, rel: &str, target: &str) -> &Self {
        let p = join_rel(&self.home, rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parents");
        }
        make_symlink(target, &p);
        self
    }

    /// Raw text of a Commons-relative file.
    pub fn contents_of_commons(&self, rel: &str) -> String {
        fs::read_to_string(join_rel(&self.commons, rel))
            .unwrap_or_else(|e| panic!("cannot read Commons/{rel}: {e}"))
    }

    /// Parse a home-relative JSON file.
    pub fn json(&self, rel: &str) -> serde_json::Value {
        let body = fs::read_to_string(join_rel(&self.home, rel))
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{rel} is not JSON: {e}"))
    }

    /// Raw bytes of a home-relative file, as text.
    pub fn contents(&self, rel: &str) -> String {
        fs::read_to_string(join_rel(&self.home, rel))
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
    }

    /// Unix permission bits of a home-relative file. Unix-only: Windows access
    /// is ACL-inherited, so there is no number here to assert on.
    #[cfg(unix)]
    pub fn mode(&self, rel: &str) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(join_rel(&self.home, rel))
            .unwrap_or_else(|e| panic!("cannot stat {rel}: {e}"))
            .permissions()
            .mode()
            & 0o777
    }

    /// Write the Commons' canonical MCP file.
    pub fn commons_mcp(&self, body: &str) -> &Self {
        self.commons_file("mcp.json", body)
    }

    /// Run with the standard overrides plus extra environment variables — the
    /// only way a test can supply a `${env:VAR}` value, since the tool never
    /// reads the real process environment.
    pub fn run_with_env(&self, args: &[&str], extra: &[(&str, &str)]) -> Outcome {
        let mut vars = vec![
            ("AGENT_SYNC_TARGET_ROOT", self.home.display().to_string()),
            ("AGENT_SYNC_HOME", self.commons.display().to_string()),
        ];
        for (k, v) in extra {
            vars.push((k, v.to_string()));
        }
        self.run_with_vars(args, &vars)
    }

    /// Snapshot of the whole home tree, for "nothing was created" assertions.
    /// Each entry is `kind:relative/path`, where kind is dir, file or link.
    pub fn tree(&self) -> BTreeSet<String> {
        let mut acc = BTreeSet::new();
        walk(&self.home, &self.home, &mut acc);
        acc
    }
}

/// Hold the global lock the way a second agent-sync process would, so a test can
/// prove a concurrent run refuses rather than racing.
pub struct HeldLock {
    _file: fs::File,
}

/// Takes the lock the same way `agent_sync::lock` does on this platform, so the
/// contention a test proves is the real one: advisory `flock` on Unix, an empty
/// share mode on Windows. Porting this rather than gating it is what keeps the
/// five busy-lock tests running on all three legs.
#[cfg(unix)]
pub fn hold_lock(path: PathBuf) -> HeldLock {
    use std::os::unix::io::AsRawFd;

    let file = open_lock_file(&path);
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(rc, 0, "test could not take the lock");
    HeldLock { _file: file }
}

#[cfg(windows)]
pub fn hold_lock(path: PathBuf) -> HeldLock {
    // The open itself is the lock: share_mode(0) makes every competing open
    // fail with ERROR_SHARING_VIOLATION until this handle closes.
    HeldLock {
        _file: open_lock_file(&path),
    }
}

fn open_lock_file(path: &Path) -> fs::File {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create lock dir");
    }
    let mut opts = fs::OpenOptions::new();
    opts.create(true).write(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(0);
    }
    opts.open(path).expect("open lock file")
}

fn walk(root: &Path, dir: &Path, acc: &mut BTreeSet<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        // symlink_metadata: a link is recorded as a link, never followed.
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).unwrap_or_default();
            acc.insert(format!("link:{rel} -> {}", target.display()));
        } else if meta.is_dir() {
            acc.insert(format!("dir:{rel}"));
            walk(root, &path, acc);
        } else {
            acc.insert(format!("file:{rel}"));
        }
    }
}
