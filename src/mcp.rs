//! MCP servers — the Rendered family (ADR-0002).
//!
//! One Commons file, `~/.agents/mcp.json`, in the de-facto standard `mcpServers`
//! shape and kept pure, is rendered into each agent's native config and merged
//! under **name identity**: a server name present in the Commons is Managed and
//! the Commons wins; any other name is Foreign and never touched, along with
//! every other key in what is usually a large file the agent owns.
//!
//! Two consequences of having no state file (ADR-0001) are load-bearing here:
//!
//! * `sync` never *deletes* a server. A name that has gone from the Commons is
//!   indistinguishable from one a user added by hand, so removal is an explicit
//!   act (`mcp remove`), not something inferred. A Disabled server is not that
//!   case: its name is still in the Commons, carrying `"disabled": true`, so
//!   removing its renders on sync rests on the same name identity as writing
//!   them did — "renders nowhere" includes renders already made.
//! * Resolved `${env:VAR}` values never reach the [`Reporter`]. Notes name the
//!   *keys* that changed, never the values, so the redaction guarantee is
//!   structural rather than a discipline applied at each call site.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::config::Config;
use crate::env::Env;
use crate::registry::{Format, Mcp, McpDialect};
use crate::target;

/// The state of one server name in one agent's config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Managed and already exactly what the Commons renders.
    Managed,
    /// In the Commons, absent from this agent's config.
    Missing,
    /// In both, but the agent's copy differs — the Commons wins.
    Drifted,
    /// Only in the agent's config. Not ours; never touched.
    Foreign,
    /// In the Commons, but this agent is not on the server's allowlist, and it is
    /// not in the agent's config either. Nothing to do.
    Excluded,
    /// In the Commons and in this agent's config, but no longer allowlisted for
    /// it. Reported so a narrowed allowlist does not leave leftovers unnoticed.
    Stranded,
    /// In the Commons with `"disabled": true`, yet still present in this
    /// agent's config. A Disabled server renders nowhere, and that includes
    /// renders already made — `sync` removes this one by name.
    Disabled,
}

impl State {
    pub fn needs_change(&self) -> bool {
        matches!(self, State::Missing | State::Drifted | State::Disabled)
    }

    /// A Foreign server is somebody else's, so it is reported but never
    /// counted as work.
    pub fn actionable(&self) -> bool {
        self.needs_change()
    }

    pub fn label(&self) -> &'static str {
        match self {
            State::Managed => "managed",
            State::Missing => "missing",
            State::Drifted => "drifted",
            State::Foreign => "foreign",
            State::Excluded => "excluded",
            State::Stranded => "stranded",
            State::Disabled => "disabled",
        }
    }
}

/// One server name in one agent's config.
///
/// `rendered` holds resolved secrets, so this type deliberately implements
/// neither `Debug` nor `Display`: there is no way to print it by accident.
#[derive(Clone)]
pub struct Item {
    pub target: String,
    pub name: String,
    /// The config file this server belongs in.
    pub path: PathBuf,
    /// Top-level key in that file holding the server map.
    pub root_key: &'static str,
    /// Wire format of that file.
    pub format: Format,
    pub state: State,
    /// What the Commons says this entry should be — resolved, never printed.
    rendered: Option<Value>,
    /// Names of the keys that differ. Key names only, never values.
    changed_keys: Vec<String>,
    /// Whether rendering this entry resolved an `${env:VAR}` reference.
    holds_secret: bool,
}

/// Everything one survey learned, including the Targets it had to skip.
#[derive(Default)]
pub struct Survey {
    pub items: Vec<Item>,
    /// Per-Target reasons a config could not be read. Advisory: the Target is
    /// left exactly as found and every other Target still syncs.
    pub skipped: Vec<String>,
    /// Faults in the Commons itself — an unparseable file, an unset
    /// `${env:VAR}`. Any entry means the whole plan is unbuildable, so `sync`
    /// stops before its first write (ADR-0007).
    pub problems: Vec<String>,
}

impl Item {
    /// A human-facing note. Safe by construction: it names keys, not values.
    pub fn note(&self) -> String {
        match self.state {
            State::Managed => String::new(),
            State::Missing => "not in this agent's config yet".into(),
            State::Drifted => {
                if self.changed_keys.is_empty() {
                    "differs from the Commons — will be restored".into()
                } else {
                    format!(
                        "differs from the Commons ({}) — will be restored",
                        self.changed_keys.join(", ")
                    )
                }
            }
            State::Foreign => "not in the Commons — left alone".into(),
            State::Excluded => "not allowlisted for this agent".into(),
            State::Stranded => {
                "still here but no longer allowlisted — `agent-sync mcp remove` clears it".into()
            }
            State::Disabled => "disabled in the Commons — will be removed".into(),
        }
    }
}

/// Something that stopped a render before anything was written.
#[derive(Debug)]
pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Survey every MCP-capable Target against the Commons.
///
/// An absent Commons file yields nothing: a family the user does not use is not
/// a problem to report. A Commons fault — an unparseable file, an unset
/// `${env:VAR}` — is *collected* into `problems` rather than returned at first
/// hit, so one run names every fault (ADR-0007); `sync` writes nothing while
/// any exist.
pub fn survey(env: &Env, config: &Config, commons_file: &Path) -> Survey {
    let mut survey = Survey::default();
    if !commons_file.is_file() {
        return survey;
    }

    let servers = match read_commons(commons_file) {
        Ok(servers) => servers,
        Err(e) => {
            survey.problems.push(e.message);
            return survey;
        }
    };

    survey_map(env, config, &servers, &mut survey);
    survey
}

/// The per-server slice of a survey: one name, one spec, every Target.
///
/// `mcp enable` uses this to prove the render works *before* the Commons write
/// lands, passing the entry as it will be once the `disabled` key is cleared.
/// Entries for other servers found in a Target's config surface as Foreign;
/// the caller filters by name.
pub fn survey_one(env: &Env, config: &Config, name: &str, spec: &Value) -> Survey {
    let mut survey = Survey::default();
    let mut servers = BTreeMap::new();
    servers.insert(name.to_string(), spec.clone());
    survey_map(env, config, &servers, &mut survey);
    survey
}

fn survey_map(env: &Env, config: &Config, servers: &BTreeMap<String, Value>, survey: &mut Survey) {
    for target in target::resolve(env, config) {
        let Some(agent) = target.agent else {
            continue;
        };
        let Mcp::KeyMerge {
            file,
            root_key,
            format,
            dialect,
        } = agent.mcp
        else {
            continue;
        };

        let path = env.in_home(file);
        let existing_servers = match read_existing(&path, root_key, format) {
            Ok(servers) => servers,
            Err(e) => {
                // This is a file agent-sync promised never to touch. One
                // unreadable config must not deny service to every other agent,
                // nor blank out a read-only report.
                survey.skipped.push(e.message);
                continue;
            }
        };

        for (name, spec) in servers {
            if is_disabled(spec) {
                // Defined but rendered nowhere. Nothing to write — and an
                // entry still present from before the server was disabled is
                // work: `sync` removes it by name.
                if existing_servers.contains_key(name) {
                    survey.items.push(Item {
                        target: target.name.clone(),
                        name: name.clone(),
                        path: path.clone(),
                        root_key,
                        format,
                        state: State::Disabled,
                        rendered: None,
                        changed_keys: Vec::new(),
                        holds_secret: false,
                    });
                }
                continue;
            }
            if !config.mcp_allows(name, &target.name) {
                // Scoped away from this agent: nothing to render, but say so if
                // an earlier sync already put it there.
                if existing_servers.contains_key(name) {
                    survey.items.push(Item {
                        target: target.name.clone(),
                        name: name.clone(),
                        path: path.clone(),
                        root_key,
                        format,
                        state: State::Stranded,
                        rendered: None,
                        changed_keys: Vec::new(),
                        holds_secret: false,
                    });
                }
                continue;
            }

            let tweaks = config.mcp_tweaks(name, &target.name);
            let rendered = match render(name, spec, dialect, env, tweaks) {
                Ok(rendered) => rendered,
                Err(e) => {
                    // Collected, not returned: a run with several unset
                    // variables names them all. Deduplicated, because the same
                    // server renders once per dialect.
                    if !survey.problems.contains(&e.message) {
                        survey.problems.push(e.message);
                    }
                    continue;
                }
            };
            let state = match existing_servers.get(name) {
                None => State::Missing,
                Some(current) if *current == rendered => State::Managed,
                Some(_) => State::Drifted,
            };
            let changed_keys = match state {
                State::Drifted => differing_keys(existing_servers.get(name), &rendered),
                _ => Vec::new(),
            };

            survey.items.push(Item {
                target: target.name.clone(),
                name: name.clone(),
                path: path.clone(),
                root_key,
                format,
                state,
                rendered: Some(rendered),
                changed_keys,
                holds_secret: mentions_env_ref(spec)
                    || tweaks.map(mentions_env_ref_map).unwrap_or(false),
            });
        }

        for name in existing_servers.keys() {
            if servers.contains_key(name) {
                continue;
            }
            survey.items.push(Item {
                target: target.name.clone(),
                name: name.clone(),
                path: path.clone(),
                root_key,
                format,
                state: State::Foreign,
                rendered: None,
                changed_keys: Vec::new(),
                holds_secret: false,
            });
        }
    }
}

/// Whether a Commons entry carries agent-sync's `"disabled": true`.
///
/// The key is agent-sync's own state on the entry, not server config — the file
/// is the interface, and `mcp enable|disable` are sugar over it. A hand-set
/// value behaves identically. `read_commons` has already refused any
/// non-boolean value, so absent and `false` are the only other shapes here.
pub fn is_disabled(spec: &Value) -> bool {
    spec.get(DISABLED).and_then(Value::as_bool).unwrap_or(false)
}

/// Whether a Commons entry references the environment, and so renders to a value
/// worth keeping out of a readable file.
fn mentions_env_ref(spec: &Value) -> bool {
    serde_json::to_string(spec)
        .map(|text| text.contains("${env:"))
        .unwrap_or(false)
}

fn mentions_env_ref_map(spec: &Map<String, Value>) -> bool {
    mentions_env_ref(&Value::Object(spec.clone()))
}

/// Write every Managed change destined for one config file, in one pass.
///
/// Grouping by file matters: an agent may receive more than one family in the
/// same file, and a write per item would clobber the previous one.
pub fn apply(
    path: &Path,
    root_key: &str,
    format: Format,
    items: &[&Item],
) -> Result<Report, Error> {
    let body = match format {
        Format::Json => render_json_document(path, root_key, items)?,
        Format::Toml => render_toml_document(path, root_key, items)?,
    };

    let written = crate::write::atomic(path, body.as_bytes()).map_err(|e| Error {
        message: format!("cannot write {}: {e}", path.display()),
    })?;

    Ok(Report {
        path: written.path,
        exposed: crate::write::is_exposed(written.mode)
            && items.iter().any(|item| item.holds_secret),
        mode: written.mode,
    })
}

/// Merge the Managed entries into a TOML config, preserving everything else.
fn render_toml_document(path: &Path, root_key: &str, items: &[&Item]) -> Result<String, Error> {
    let text = read_text(path)?;
    let mut entries = Map::new();
    for item in items {
        if let Some(rendered) = &item.rendered {
            entries.insert(item.name.clone(), rendered.clone());
        }
    }

    // toml_edit normalises both of these away; the user's file gets them back.
    let had_bom = text.starts_with('\u{feff}');
    let had_crlf = text.contains("\r\n");
    let body = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let merged =
        crate::mcp_toml::merge_section(body, root_key, &entries).map_err(|detail| Error {
            message: format!("{}: {detail} — left untouched", path.display()),
        })?;

    let merged = if had_crlf {
        merged.replace('\n', "\r\n")
    } else {
        merged
    };
    Ok(if had_bom {
        format!("\u{feff}{merged}")
    } else {
        merged
    })
}

/// Merge the Managed entries into a JSON config, preserving everything else.
fn render_json_document(path: &Path, root_key: &str, items: &[&Item]) -> Result<String, Error> {
    let mut document = read_agent_config(path)?;

    // An existing document that is not an object is somebody else's data in a
    // shape we do not understand. Replacing it with `{}` would destroy it.
    if !document.is_null() && !document.is_object() {
        return Err(Error {
            message: format!(
                "{} does not hold a JSON object — left untouched",
                path.display()
            ),
        });
    }
    if document.is_null() {
        document = Value::Object(Map::new());
    }
    let root = document.as_object_mut().expect("checked above");

    let mut servers = match root.get(root_key) {
        None => Map::new(),
        Some(Value::Object(existing)) => existing.clone(),
        Some(_) => {
            return Err(Error {
                message: format!(
                    "{}: `{root_key}` is not an object — left untouched",
                    path.display()
                ),
            });
        }
    };

    for item in items {
        let Some(rendered) = &item.rendered else {
            continue;
        };
        // Replace the whole subtree: a key the Commons dropped must disappear,
        // which a deep merge would preserve forever.
        servers.insert(item.name.clone(), rendered.clone());
    }

    root.insert(root_key.to_string(), Value::Object(servers));

    serde_json::to_string_pretty(&document).map_err(|e| Error {
        message: format!("cannot serialise {}: {e}", path.display()),
    })
}

/// What one write produced, for the caller to report on.
pub struct Report {
    /// The file that actually received the bytes, after following any symlink.
    pub path: PathBuf,
    /// Resolved secrets landed in a file others can read.
    pub exposed: bool,
    pub mode: u32,
}

/// The Commons' servers, keyed by name.
///
/// One shape only, the de-facto standard `{"mcpServers": {...}}`. Falling back
/// to "treat every top-level key as a server" would quietly turn a `$schema`
/// line into a server named `$schema`, and would silently ignore real servers
/// in a file that merely had a typo in the wrapper.
fn read_commons(path: &Path) -> Result<BTreeMap<String, Value>, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| Error {
        message: format!("cannot read {}: {e}", path.display()),
    })?;
    let parsed: Value = serde_json::from_str(&text).map_err(|e| Error {
        message: format!("{} is not valid JSON: {e}", path.display()),
    })?;

    let map = parsed
        .get("mcpServers")
        .and_then(Value::as_object)
        .ok_or_else(|| Error {
            message: format!(
                "{} must hold an `mcpServers` object at the top level",
                path.display()
            ),
        })?;

    for (name, spec) in map {
        if !spec.is_object() {
            return Err(Error {
                message: format!("{}: server `{name}` must be an object", path.display()),
            });
        }
        // `disabled` is agent-sync's key, so its shape is agent-sync's to
        // police: anything but a boolean has no meaning to fall back on.
        if let Some(flag) = spec.get(DISABLED)
            && !flag.is_boolean()
        {
            return Err(Error {
                message: format!(
                    "{}: server `{name}`: `disabled` must be true or false",
                    path.display()
                ),
            });
        }
    }

    Ok(map
        .iter()
        .map(|(name, spec)| (name.clone(), spec.clone()))
        .collect())
}

/// The servers already in an agent's config, whatever format it is written in.
///
/// TOML is normalised to JSON here purely so the survey can compare entries
/// with one set of rules; the JSON form is never written back.
/// The servers already in one agent's config, whatever format it is in.
pub fn native_servers(
    path: &Path,
    root_key: &str,
    format: Format,
) -> Result<Map<String, Value>, Error> {
    read_existing(path, root_key, format)
}

fn read_existing(path: &Path, root_key: &str, format: Format) -> Result<Map<String, Value>, Error> {
    match format {
        Format::Json => match read_agent_config(path)?.get(root_key) {
            None => Ok(Map::new()),
            Some(Value::Object(servers)) => Ok(servers.clone()),
            // Treating this as "no servers" would report every Commons server as
            // missing forever, while the write refuses every time.
            Some(_) => Err(Error {
                message: format!(
                    "{}: `{root_key}` is not an object — left untouched",
                    path.display()
                ),
            }),
        },
        Format::Toml => {
            let text = read_text(path)?;
            crate::mcp_toml::read_section(&text, root_key).map_err(|detail| Error {
                message: format!("{}: {detail} — left untouched", path.display()),
            })
        }
    }
}

/// A file's text, treating "not there yet" as empty.
fn read_text(path: &Path) -> Result<String, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(Error {
            message: format!("cannot read {}: {e}", path.display()),
        }),
    }
}

/// Parse an agent's config, treating "not there yet" as an empty document.
fn read_agent_config(path: &Path) -> Result<Value, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(text) => serde_json::from_str(&text).map_err(|e| Error {
            // The message names the file and the position, never the content:
            // this file may hold resolved secrets.
            message: format!(
                "{} is not valid JSON (line {}, column {}) — left untouched",
                path.display(),
                e.line(),
                e.column()
            ),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(Error {
            message: format!("cannot read {}: {e}", path.display()),
        }),
    }
}

/// How a server talks, whether or not the Commons said so outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Stdio,
    Http,
    Sse,
}

/// An absent `type` is inferred the way every agent infers it: a server with a
/// URL and no command is remote.
fn transport(spec: &Map<String, Value>) -> Transport {
    match spec.get("type").and_then(Value::as_str) {
        Some("http") => Transport::Http,
        Some("sse") => Transport::Sse,
        Some("stdio") => Transport::Stdio,
        _ if spec.contains_key("url") && !spec.contains_key("command") => Transport::Http,
        _ => Transport::Stdio,
    }
}

/// Keys every dialect handles itself, and so must not pass through verbatim.
const TRANSLATED: &[&str] = &["type", "command", "args", "env", "url", "headers"];

/// The one key on a Commons entry that is agent-sync's rather than the
/// server's. It marks the entry Disabled and must never reach a render.
pub const DISABLED: &str = "disabled";

/// Copy the keys agent-sync does not model, so an agent-specific setting the
/// user wrote in the Commons still reaches its agent.
fn passthrough(spec: &Map<String, Value>, out: &mut Map<String, Value>) {
    for (key, value) in spec {
        if !TRANSLATED.contains(&key.as_str()) {
            out.insert(key.clone(), value.clone());
        }
    }
}

/// Render one Commons server into an agent's native shape, resolving references.
fn render(
    name: &str,
    spec: &Value,
    dialect: McpDialect,
    env: &Env,
    tweaks: Option<&Map<String, Value>>,
) -> Result<Value, Error> {
    render_inner(name, spec, dialect, env, tweaks, true)
}

/// Render without resolving `${env:VAR}`, for comparing translations rather
/// than values. A literal reference in an agent's own config must compare equal
/// to itself, or adoption would call every such server unfaithful.
fn render_structure(
    name: &str,
    spec: &Value,
    dialect: McpDialect,
    env: &Env,
    tweaks: Option<&Map<String, Value>>,
) -> Result<Value, Error> {
    render_inner(name, spec, dialect, env, tweaks, false)
}

fn render_inner(
    name: &str,
    spec: &Value,
    dialect: McpDialect,
    env: &Env,
    tweaks: Option<&Map<String, Value>>,
    resolve: bool,
) -> Result<Value, Error> {
    let mut rendered = render_dialect(name, spec, dialect, env, resolve)?;

    // Tweaks are native keys, so they land after translation — and before the
    // Managed/Drifted comparison, or a tweaked server would drift forever.
    if let Some(tweaks) = tweaks {
        let raw = Value::Object(tweaks.clone());
        let resolved = if resolve {
            resolve_refs(name, &raw, env)?
        } else {
            raw
        };
        if let (Some(target), Some(extra)) = (rendered.as_object_mut(), resolved.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
        if dialect == McpDialect::Codex {
            strip_nulls(&mut rendered);
        }
    }

    Ok(rendered)
}

fn render_dialect(
    name: &str,
    spec: &Value,
    dialect: McpDialect,
    env: &Env,
    resolve: bool,
) -> Result<Value, Error> {
    let mut resolved = if resolve {
        resolve_refs(name, spec, env)?
    } else {
        spec.clone()
    };
    // `disabled` marks the Commons entry, it does not configure the server:
    // no dialect translates it, and passthrough must not carry it either.
    if let Some(fields) = resolved.as_object_mut() {
        fields.shift_remove(DISABLED);
    }
    let Some(fields) = resolved.as_object() else {
        return Ok(resolved);
    };
    Ok(Value::Object(match dialect {
        // The Commons is already in this shape, so there is nothing to translate.
        McpDialect::Standard => return Ok(resolved),
        McpDialect::Codex => {
            // The TOML merge drops null-valued keys because TOML has no null.
            // Dropping them from the rendered entry too keeps the comparison
            // symmetric with what the file can actually hold — otherwise the
            // entry reads back different every time and never converges.
            let mut out = Value::Object(render_codex(fields));
            strip_nulls(&mut out);
            return Ok(out);
        }
        McpDialect::Opencode => render_opencode(fields),
        McpDialect::Gemini => render_gemini(fields),
        McpDialect::Windsurf => render_windsurf(fields),
    }))
}

/// Codex has one remote transport and no `type` key, and calls headers
/// `http_headers`.
fn render_codex(spec: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    match transport(spec) {
        Transport::Stdio => {
            copy(spec, "command", &mut out);
            copy(spec, "args", &mut out);
            copy(spec, "env", &mut out);
        }
        _ => {
            copy(spec, "url", &mut out);
            if let Some(headers) = spec.get("headers") {
                out.insert("http_headers".into(), headers.clone());
            }
        }
    }
    passthrough(spec, &mut out);
    out
}

/// OpenCode splits servers into local and remote, takes the executable and its
/// arguments as one array, and calls the environment `environment`.
fn render_opencode(spec: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    match transport(spec) {
        Transport::Stdio => {
            out.insert("type".into(), Value::from("local"));
            let mut command = Vec::new();
            if let Some(program) = spec.get("command").and_then(Value::as_str) {
                command.push(Value::from(program));
            }
            if let Some(args) = spec.get("args").and_then(Value::as_array) {
                command.extend(args.iter().cloned());
            }
            if !command.is_empty() {
                out.insert("command".into(), Value::Array(command));
            }
            if let Some(env) = spec.get("env") {
                out.insert("environment".into(), env.clone());
            }
        }
        _ => {
            out.insert("type".into(), Value::from("remote"));
            copy(spec, "url", &mut out);
            copy(spec, "headers", &mut out);
        }
    }
    passthrough(spec, &mut out);
    out
}

/// Gemini encodes the remote transport in the key name: `url` is SSE,
/// `httpUrl` is streamable HTTP.
fn render_gemini(spec: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    match transport(spec) {
        Transport::Stdio => {
            copy(spec, "command", &mut out);
            copy(spec, "args", &mut out);
            copy(spec, "env", &mut out);
        }
        remote => {
            let key = if remote == Transport::Sse {
                "url"
            } else {
                "httpUrl"
            };
            if let Some(url) = spec.get("url") {
                out.insert(key.into(), url.clone());
            }
            copy(spec, "headers", &mut out);
        }
    }
    passthrough(spec, &mut out);
    out
}

/// Windsurf calls the remote URL `serverUrl` and has no `type` key.
fn render_windsurf(spec: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    match transport(spec) {
        Transport::Stdio => {
            copy(spec, "command", &mut out);
            copy(spec, "args", &mut out);
            copy(spec, "env", &mut out);
        }
        _ => {
            if let Some(url) = spec.get("url") {
                out.insert("serverUrl".into(), url.clone());
            }
            copy(spec, "headers", &mut out);
        }
    }
    passthrough(spec, &mut out);
    out
}

/// Remove null-valued keys, recursively.
fn strip_nulls(value: &mut Value) {
    if let Value::Object(fields) = value {
        fields.retain(|_, v| !v.is_null());
        for nested in fields.values_mut() {
            strip_nulls(nested);
        }
    }
}

fn copy(spec: &Map<String, Value>, key: &str, out: &mut Map<String, Value>) {
    if let Some(value) = spec.get(key) {
        out.insert(key.to_string(), value.clone());
    }
}

/// Replace every `${env:VAR}` in every string with its value.
///
/// Fails on the first unset variable, so a partially resolved entry can never
/// reach a file.
fn resolve_refs(server: &str, value: &Value, env: &Env) -> Result<Value, Error> {
    match value {
        Value::String(text) => Ok(Value::String(expand(server, text, env)?)),
        Value::Array(items) => items
            .iter()
            .map(|v| resolve_refs(server, v, env))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(fields) => {
            let mut out = Map::new();
            for (key, v) in fields {
                out.insert(key.clone(), resolve_refs(server, v, env)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// Expand `${env:VAR}` occurrences in one string.
fn expand(server: &str, text: &str, env: &Env) -> Result<String, Error> {
    const OPEN: &str = "${env:";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find(OPEN) {
        let (before, from_marker) = rest.split_at(start);
        out.push_str(before);

        let body = &from_marker[OPEN.len()..];
        let Some(end) = body.find('}') else {
            // An unterminated reference is literal text, not an error.
            out.push_str(from_marker);
            return Ok(out);
        };

        let name = &body[..end];
        let Some(value) = env.var(name).filter(|v| !v.is_empty()) else {
            return Err(Error {
                message: format!(
                    "mcp server `{server}`: ${{env:{name}}} is not set in the environment"
                ),
            });
        };
        out.push_str(value);
        rest = &body[end + 1..];
    }

    out.push_str(rest);
    Ok(out)
}

/// Which top-level keys of an entry differ. Names only — never values.
fn differing_keys(current: Option<&Value>, rendered: &Value) -> Vec<String> {
    let empty = Map::new();
    let current = current.and_then(Value::as_object).unwrap_or(&empty);
    let rendered = rendered.as_object().unwrap_or(&empty);

    let mut keys: Vec<String> = rendered
        .iter()
        .filter(|(key, value)| current.get(*key) != Some(*value))
        .map(|(key, _)| key.clone())
        .chain(
            current
                .keys()
                .filter(|key| !rendered.contains_key(*key))
                .cloned(),
        )
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// What adopting one native entry produced.
///
/// Like [`Item`], this deliberately implements neither `Debug` nor `Display`:
/// `canonical` and `tweaks` hold values read out of an agent's config, which
/// may be secrets the user put there by hand.
pub struct Absorbed {
    /// The entry as it will be written to the Commons, in the standard shape.
    pub canonical: Value,
    /// Native keys with no canonical home, kept for the source agent alone.
    pub tweaks: Map<String, Value>,
    /// What could not be represented faithfully, in words, never values.
    pub notes: Vec<String>,
}

/// Translate one agent's native entry back into the canonical shape.
///
/// Keys agent-sync models become canonical; everything else becomes a Tweak for
/// the agent it came from, because a knob one agent understands is not
/// automatically meaningful to the others.
pub fn absorb(name: &str, entry: &Value, dialect: McpDialect) -> Result<Absorbed, Error> {
    let fields = entry.as_object().ok_or_else(|| Error {
        message: format!("server `{name}` is not an object"),
    })?;

    let mut canonical = Map::new();
    let mut tweaks = Map::new();
    let mut notes = Vec::new();
    // Native keys this dialect consumed, so they are not also kept as Tweaks.
    let mut consumed: Vec<&str> = Vec::new();

    match dialect {
        McpDialect::Standard => {
            for key in TRANSLATED {
                if let Some(value) = fields.get(*key) {
                    canonical.insert((*key).to_string(), value.clone());
                    consumed.push(key);
                }
            }
        }
        McpDialect::Codex => {
            if let Some(url) = fields.get("url") {
                canonical.insert("url".into(), url.clone());
                consumed.push("url");
                if let Some(headers) = fields.get("http_headers") {
                    canonical.insert("headers".into(), headers.clone());
                    consumed.push("http_headers");
                }
                notes.push(format!(
                    "`{name}`: Codex has one remote transport, so http and sse cannot be told \
                     apart — no `type` was recorded"
                ));
            } else {
                for key in ["command", "args", "env"] {
                    if let Some(value) = fields.get(key) {
                        canonical.insert(key.to_string(), value.clone());
                        consumed.push(key);
                    }
                }
            }
        }
        McpDialect::Opencode => {
            let kind = fields.get("type").and_then(Value::as_str);
            consumed.push("type");
            let remote = kind == Some("remote") || (kind.is_none() && fields.contains_key("url"));
            if remote {
                for key in ["url", "headers"] {
                    if let Some(value) = fields.get(key) {
                        canonical.insert(key.to_string(), value.clone());
                        consumed.push(key);
                    }
                }
                notes.push(format!(
                    "`{name}`: OpenCode has one remote transport, so http and sse cannot be told \
                     apart — no `type` was recorded"
                ));
            } else {
                match fields.get("command") {
                    Some(Value::Array(parts)) => {
                        let mut parts = parts.iter();
                        if let Some(program) = parts.next() {
                            canonical.insert("command".into(), program.clone());
                        }
                        let args: Vec<Value> = parts.cloned().collect();
                        if !args.is_empty() {
                            canonical.insert("args".into(), Value::Array(args));
                        }
                        consumed.push("command");
                    }
                    // A hand-written config may use a bare string; take it as
                    // written rather than dropping the server on the floor.
                    Some(other) => {
                        canonical.insert("command".into(), other.clone());
                        consumed.push("command");
                    }
                    None => {}
                }
                if let Some(env) = fields.get("environment") {
                    canonical.insert("env".into(), env.clone());
                    consumed.push("environment");
                }
            }
        }
        McpDialect::Gemini => {
            // Gemini is the one dialect that records the transport, in the name
            // of the URL key, so adoption from it is exact.
            if let Some(url) = fields.get("httpUrl") {
                canonical.insert("type".into(), Value::from("http"));
                canonical.insert("url".into(), url.clone());
                consumed.push("httpUrl");
            } else if let Some(url) = fields.get("url") {
                canonical.insert("type".into(), Value::from("sse"));
                canonical.insert("url".into(), url.clone());
                consumed.push("url");
            }
            for key in ["command", "args", "env", "headers"] {
                if let Some(value) = fields.get(key) {
                    canonical.insert(key.to_string(), value.clone());
                    consumed.push(key);
                }
            }
        }
        McpDialect::Windsurf => {
            if let Some(url) = fields.get("serverUrl") {
                canonical.insert("url".into(), url.clone());
                consumed.push("serverUrl");
                notes.push(format!(
                    "`{name}`: Windsurf records no transport, so http and sse cannot be told \
                     apart — no `type` was recorded"
                ));
            }
            for key in ["command", "args", "env", "headers"] {
                if let Some(value) = fields.get(key) {
                    canonical.insert(key.to_string(), value.clone());
                    consumed.push(key);
                }
            }
        }
    }

    for (key, value) in fields {
        if consumed.contains(&key.as_str()) {
            continue;
        }
        tweaks.insert(key.clone(), value.clone());
    }

    // A null cannot survive the TOML config file, so it is never claimed as a
    // Tweak — saying it was kept and then losing it is the worst outcome.
    let dropped: Vec<String> = tweaks
        .iter()
        .filter(|(_, v)| v.is_null())
        .map(|(k, _)| k.clone())
        .collect();
    if !dropped.is_empty() {
        tweaks.retain(|_, v| !v.is_null());
        notes.push(format!(
            "`{name}`: {} had no value and was not kept",
            dropped.join(", ")
        ));
    }

    if !tweaks.is_empty() {
        let mut names: Vec<&str> = tweaks.keys().map(String::as_str).collect();
        names.sort();
        notes.push(format!(
            "`{name}`: {} has no place in the standard shape and was kept as a Tweak for this \
             agent alone",
            names.join(", ")
        ));
    }

    Ok(Absorbed {
        canonical: Value::Object(canonical),
        tweaks,
        notes,
    })
}

/// Which keys would still differ if this adoption were synced back.
///
/// Empty means the next `sync` is genuinely a no-op for this server, which is
/// the only proof that an adoption was faithful.
pub fn verify(
    name: &str,
    canonical: &Value,
    dialect: McpDialect,
    native: &Value,
    env: &Env,
    tweaks: Option<&Map<String, Value>>,
) -> Result<Vec<String>, Error> {
    let rendered = render_structure(name, canonical, dialect, env, tweaks)?;
    Ok(differing_keys(Some(native), &rendered))
}

/// Remove one server from a Target's config. The only place agent-sync deletes.
///
/// Returns `None` when the server was not there, so the caller can stay quiet.
pub fn strip(
    path: &Path,
    root_key: &str,
    format: Format,
    name: &str,
) -> Result<Option<Report>, Error> {
    let body = match format {
        Format::Json => {
            let mut document = read_agent_config(path)?;
            let Some(root) = document.as_object_mut() else {
                return Ok(None);
            };
            let Some(Value::Object(servers)) = root.get_mut(root_key) else {
                return Ok(None);
            };
            // shift_remove, never remove: with preserve_order the latter swaps
            // the last entry into the hole and scrambles the user's file.
            if servers.shift_remove(name).is_none() {
                return Ok(None);
            }
            serde_json::to_string_pretty(&document).map_err(|e| Error {
                message: format!("cannot serialise {}: {e}", path.display()),
            })?
        }
        Format::Toml => {
            let text = read_text(path)?;
            match crate::mcp_toml::remove_entry(&text, root_key, name).map_err(|detail| Error {
                message: format!("{}: {detail} — left untouched", path.display()),
            })? {
                Some(updated) => updated,
                None => return Ok(None),
            }
        }
    };

    let written = crate::write::atomic(path, body.as_bytes()).map_err(|e| Error {
        message: format!("cannot write {}: {e}", path.display()),
    })?;
    Ok(Some(Report {
        path: written.path,
        exposed: false,
        mode: written.mode,
    }))
}

/// Read the Commons' servers, or an empty map if there is no Commons file yet.
pub fn commons_servers(commons_file: &Path) -> Result<Map<String, Value>, Error> {
    if !commons_file.is_file() {
        return Ok(Map::new());
    }
    Ok(read_commons(commons_file)?.into_iter().collect())
}

/// Write the Commons' servers back, in the standard shape.
pub fn commons_write(commons_file: &Path, servers: &Map<String, Value>) -> Result<(), Error> {
    let mut document = Map::new();
    document.insert("mcpServers".into(), Value::Object(servers.clone()));
    let body = serde_json::to_string_pretty(&Value::Object(document)).map_err(|e| Error {
        message: format!("cannot serialise {}: {e}", commons_file.display()),
    })?;
    crate::write::atomic(commons_file, format!("{body}\n").as_bytes()).map_err(|e| Error {
        message: format!("cannot write {}: {e}", commons_file.display()),
    })?;
    Ok(())
}
