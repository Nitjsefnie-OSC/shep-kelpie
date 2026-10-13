//! `shep kelpie get` and `set`: one setting of a project's `[app.dogs.kelpie]` table
//!
//! The table is stored on the project's runner sheep. `set` reads the whole
//! table, changes the one dotted key, checks the result as the runner will
//! read it, and only then writes the whole table back. A runner that is
//! running takes the change at its next wake.

use std::path::Path;

use serde_json::{Map, Value};
use shep_client::Client;
use shep_client::shep_core::config::DogTable;
use shep_client::shep_core::protocol::Request;
use shep_client::shep_core::protocol::request::Response;

use super::send;
use crate::runner::ProjectName;
use crate::settings::Settings;
use crate::shepherd::{self, DOG};

/// Where a project's relative paths and `~/` start, as the runner reads its settings
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    /// The maintainer's home
    pub home: &'a Path,
    /// The project's own folder under kelpie's home
    pub folder: &'a Path,
}

/// The project's table, or one dotted key's value, as TOML lines
///
/// # Errors
///
/// A message when the shepherd cannot be read, the project has no table (it
/// points at `add`), or `key` is not set in it.
pub async fn get(
    client: &Client,
    name: &ProjectName,
    key: Option<&str>,
) -> Result<Vec<String>, String> {
    let table = table_of_project(client, name).await?;
    let Some(key) = key else {
        return Ok(lines(&render_table(&table)?));
    };
    let value = lookup(&table, key)
        .ok_or_else(|| format!("`{key}` is not set in project {name}'s settings"))?;
    Ok(lines(&render_value(value)?))
}

/// Sets one dotted `key` of the project's table to `value`, and says what it wrote
///
/// `value` is read as a TOML value, and as a string where it is not one.
///
/// # Errors
///
/// A message when the shepherd cannot be read or written, the project has no
/// table (it points at `add`), `key` runs through a value that is not a
/// table, or the changed table is not settings the runner would read: the
/// message is the reason it gives, and nothing is written.
pub async fn set(
    client: &Client,
    name: &ProjectName,
    place: Place<'_>,
    key: &str,
    value: &str,
) -> Result<Vec<String>, String> {
    let mut table = table_of_project(client, name).await?;
    let value = read_value(value);
    let shown = render_value(&value)?;
    insert(&mut table, key, value)?;
    Settings::from_table(&table, name.as_str(), place.home, place.folder)
        .map_err(|e| e.to_string())?;
    let request = Request::SetSheepDogSettings {
        name: name.as_str().to_owned(),
        dog: DOG.to_owned(),
        table: Some(DogTable::from(table)),
    };
    send(client, request, |r| {
        matches!(r, Response::SheepDogSettingsSet { .. })
    })
    .await?;
    Ok(vec![format!(
        "project {name}: {key} = {shown}; a running runner takes it at its next wake"
    )])
}

async fn table_of_project(
    client: &Client,
    name: &ProjectName,
) -> Result<Map<String, Value>, String> {
    let tables = shepherd::read_tables_with(client, name.as_str()).await?;
    tables.project.ok_or_else(|| {
        format!("project {name} has no settings: `shep kelpie add {name}` registers it")
    })
}

// `text` as a TOML value, or as a string where it is not one. A date is text too.
fn read_value(text: &str) -> Value {
    let parsed = toml::from_str::<toml::Table>(&format!("value = {text}"))
        .ok()
        .and_then(|mut table| table.remove("value"));
    match parsed {
        Some(toml::Value::Datetime(_)) | None => Value::String(text.to_owned()),
        Some(value) => {
            serde_json::to_value(value).unwrap_or_else(|_| Value::String(text.to_owned()))
        }
    }
}

fn lookup<'a>(table: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.as_object()?.get(part)?;
    }
    Some(value)
}

// Puts `value` at `key`, making the tables on the way that are missing.
fn insert(table: &mut Map<String, Value>, key: &str, value: Value) -> Result<(), String> {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().filter(|last| !last.is_empty());
    let (Some(last), true) = (last, parts.iter().all(|part| !part.is_empty())) else {
        return Err(format!("`{key}` is not a dotted key like `git.merging`"));
    };
    let mut at = table;
    let mut walked = Vec::new();
    for part in parts {
        walked.push(part);
        let entry = at.entry(part).or_insert_with(|| Value::Object(Map::new()));
        at = entry.as_object_mut().ok_or_else(|| {
            format!(
                "`{}` is not a table, so `{key}` is not in it",
                walked.join(".")
            )
        })?;
    }
    at.insert(last.to_owned(), value);
    Ok(())
}

fn render_table(table: &Map<String, Value>) -> Result<String, String> {
    let table: toml::Table =
        serde_json::from_value(Value::Object(table.clone())).map_err(|e| e.to_string())?;
    toml::to_string(&table).map_err(|e| e.to_string())
}

// A table value as a table, anything else as the TOML a `key = ` takes.
fn render_value(value: &Value) -> Result<String, String> {
    if let Value::Object(table) = value {
        return render_table(table);
    }
    let mut holder = Map::new();
    holder.insert("value".to_owned(), value.clone());
    let text = render_table(&holder)?;
    let shown = text.strip_prefix("value = ").unwrap_or(&text);
    Ok(shown.trim_end().to_owned())
}

fn lines(text: &str) -> Vec<String> {
    text.trim_end().lines().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests;
