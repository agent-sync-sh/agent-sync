//! TOML side of the MCP family.
//!
//! Codex keeps its MCP servers in the same file as its model settings, project
//! trust levels, plugin enablement and hook trust hashes — and that file has
//! comments in it. So this is a *format-preserving* merge: `toml_edit` keeps
//! everything it was not asked to change, down to whitespace, and agent-sync
//! replaces exactly the `[mcp_servers.<name>]` tables it owns.
//!
//! Reading and writing are deliberately asymmetric. Existing entries are
//! converted to JSON purely so the survey can compare them against a rendered
//! entry with one set of rules; the conversion is never written back.

use serde_json::{Map, Value};
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue};

/// Describe a parse failure by position only.
///
/// `TomlError`'s own `Display` quotes the offending source line verbatim, and
/// these files hold resolved secrets — so it must never reach a message.
pub fn position_only(text: &str, error: &toml_edit::TomlError) -> String {
    let offset = error.span().map_or(0, |s| s.start).min(text.len());
    let head = &text.as_bytes()[..offset];
    let line = 1 + head.iter().filter(|b| **b == b'\n').count();
    let column = 1 + head.len() - head.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    format!("invalid TOML (line {line}, column {column})")
}

/// Read the servers under `section` as JSON, for comparison only.
pub fn read_section(text: &str, section: &str) -> Result<Map<String, Value>, String> {
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| position_only(text, &e))?;

    let Some(item) = doc.get(section) else {
        return Ok(Map::new());
    };
    let Some(table) = item.as_table() else {
        return Err(format!("`{section}` is not a table"));
    };

    let mut out = Map::new();
    for (name, entry) in table.iter() {
        out.insert(name.to_string(), item_to_json(entry));
    }
    Ok(out)
}

/// Merge `entries` into `section`, leaving every other byte of `text` alone.
pub fn merge_section(
    text: &str,
    section: &str,
    entries: &Map<String, Value>,
) -> Result<String, String> {
    let mut doc: DocumentMut = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse()
            .map_err(|e: toml_edit::TomlError| position_only(text, &e))?
    };

    let root = doc.as_table_mut();
    if root.get(section).is_none() {
        let mut parent = Table::new();
        // Implicit: `[mcp_servers]` itself is never emitted as a bare header,
        // only `[mcp_servers.<name>]`.
        parent.set_implicit(true);
        root.insert(section, Item::Table(parent));
    }

    // Deliberately no `set_implicit` here: the creation branch above already
    // made the table we inserted implicit, and a parent toml_edit inferred from
    // an existing `[mcp_servers.<name>]` is implicit already. Forcing it would
    // delete a bare `[mcp_servers]` header the user wrote by hand.
    let parent = root
        .get_mut(section)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("`{section}` is not a table"))?;

    for (name, entry) in entries {
        let object = entry
            .as_object()
            .ok_or_else(|| format!("server `{name}` is not an object"))?;
        let mut table = object_to_table(object)?;
        table.set_implicit(false);

        // Keep the table where the user had it and carry its decor across, so
        // a merge neither shuffles their file nor eats the comment they wrote
        // above their own server.
        if let Some(previous) = parent.get(name).and_then(Item::as_table) {
            if let Some(position) = previous.position() {
                table.set_position(Some(position));
            }
            *table.decor_mut() = previous.decor().clone();
        }

        // Reusing the existing key preserves its formatting too.
        match parent.key(name).cloned() {
            Some(key) => {
                parent.insert_formatted(&key, Item::Table(table));
            }
            None => {
                parent.insert(name, Item::Table(table));
            }
        }
    }

    Ok(doc.to_string())
}

/// Remove one entry from a section, preserving everything else.
///
/// Returns `None` when the entry was not there.
pub fn remove_entry(text: &str, section: &str, name: &str) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| position_only(text, &e))?;

    let Some(parent) = doc.get_mut(section).and_then(Item::as_table_mut) else {
        return Ok(None);
    };
    // `remove` on a toml_edit table is order-preserving, unlike the JSON side.
    if parent.remove(name).is_none() {
        return Ok(None);
    }
    Ok(Some(doc.to_string()))
}

/// A JSON object as a standard (non-inline) TOML table.
pub fn object_to_table(object: &Map<String, Value>) -> Result<Table, String> {
    let mut table = Table::new();
    for (key, value) in object {
        match value {
            // A nested object becomes its own `[a.b]` section rather than an
            // inline table, matching how agents write these files by hand.
            Value::Object(nested) => {
                let mut child = object_to_table(nested)?;
                child.set_implicit(false);
                table.insert(key, Item::Table(child));
            }
            // TOML has no null. Dropping the key is the only faithful choice.
            Value::Null => {}
            other => {
                let converted = to_toml_value(other)
                    .ok_or_else(|| format!("`{key}` has a value TOML cannot hold"))?;
                table.insert(key, Item::Value(converted));
            }
        }
    }
    Ok(table)
}

fn to_toml_value(value: &Value) -> Option<TomlValue> {
    Some(match value {
        Value::String(s) => TomlValue::from(s.as_str()),
        Value::Bool(b) => TomlValue::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TomlValue::from(i)
            } else if n.as_u64().is_some() {
                // TOML integers are signed 64-bit. Rendering this as a float
                // would corrupt it and then drift forever.
                return None;
            } else {
                TomlValue::from(n.as_f64()?)
            }
        }
        Value::Array(items) => {
            let mut array = Array::new();
            for item in items {
                // An array of tables inside a server entry is not something the
                // MCP shape uses; refuse rather than silently flatten it.
                array.push(to_toml_value(item)?);
            }
            TomlValue::Array(array)
        }
        // Objects are handled by the caller as sub-tables; nulls cannot exist.
        Value::Object(_) | Value::Null => return None,
    })
}

/// A TOML table as a JSON object — used for Tweak tables in the tool config.
pub fn table_to_json(table: &Table) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, item) in table.iter() {
        out.insert(key.to_string(), item_to_json(item));
    }
    out
}

/// Convert a parsed TOML item back to JSON, for comparison only.
fn item_to_json(item: &Item) -> Value {
    match item {
        Item::Value(value) => value_to_json(value),
        Item::Table(table) => {
            let mut out = Map::new();
            for (key, child) in table.iter() {
                out.insert(key.to_string(), item_to_json(child));
            }
            Value::Object(out)
        }
        Item::ArrayOfTables(tables) => Value::Array(
            tables
                .iter()
                .map(|t| item_to_json(&Item::Table(t.clone())))
                .collect(),
        ),
        Item::None => Value::Null,
    }
}

fn value_to_json(value: &TomlValue) -> Value {
    match value {
        TomlValue::String(s) => Value::String(s.value().clone()),
        TomlValue::Integer(i) => Value::from(*i.value()),
        TomlValue::Float(f) => Value::from(*f.value()),
        TomlValue::Boolean(b) => Value::from(*b.value()),
        TomlValue::Datetime(d) => Value::String(d.value().to_string()),
        TomlValue::Array(items) => Value::Array(items.iter().map(value_to_json).collect()),
        TomlValue::InlineTable(table) => {
            let mut out = Map::new();
            for (key, child) in table.iter() {
                out.insert(key.to_string(), value_to_json(child));
            }
            Value::Object(out)
        }
    }
}
