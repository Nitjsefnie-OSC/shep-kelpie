//! `shep kelpie get` and `set`: one setting of a project's `[app.dogs.kelpie]` table
//!
//! The table is stored on the project's runner sheep. `set` reads the whole
//! table, changes the one dotted key, checks the result as the runner will
//! read it, and only then writes the whole table back. A runner that is
//! running takes the change at its next wake.

use std::path::Path;

use serde_json::{Map, Value};
use shep_client::shep_core::config::DogTable;
use shep_client::shep_core::protocol::Request;
use shep_client::shep_core::protocol::request::Response;

use super::send;
use super::share::share_none;
use crate::agents::Agents;
use crate::runner::{ProjectName, ProjectPaths, check_gateways};
use crate::settings::{Settings, SettingsError, source};
use crate::shepherd::{self, Connection, DOG, Tables};

/// What `set` reads a project's settings against, as its runner does
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    /// The maintainer's home, for `~/` in settings
    pub home: &'a Path,
    /// The project's own folders under kelpie's home
    pub paths: &'a ProjectPaths,
}

/// The project's table, or one dotted key's value, as TOML lines
///
/// # Errors
///
/// A message when the shepherd cannot be read, the project has no table (it
/// points at `add`), or `key` is not set in it.
pub async fn get(
    client: &dyn Connection,
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
/// table, or the changed table is settings the runner would refuse to start
/// or reread on: the message is its reason, and nothing is written. Moving
/// `git.checkout` or `git.remote` onto a checkout or repo another project
/// runs is refused as `add` refuses it.
pub async fn set(
    client: &dyn Connection,
    name: &ProjectName,
    place: Place<'_>,
    key: &str,
    value: &str,
) -> Result<Vec<String>, String> {
    let read = shepherd::read_tables_with(client, name.as_str()).await?;
    let before = read.project.clone().ok_or_else(|| no_table(name))?;
    let mut table = before.clone();
    let value = read_value(value);
    let shown = render_value(&value)?;
    insert(&mut table, key, value)?;
    let git_moved = ["git.checkout", "git.remote"]
        .iter()
        .any(|moved| lookup(&before, moved) != lookup(&table, moved));
    let changed = Tables {
        project: Some(table),
        kelpie: read.kelpie,
    };
    let settings = runnable(&changed, name, place)?;
    if git_moved {
        let remote = crate::runner::check_checkout(&settings).map_err(|e| e.to_string())?;
        let (rows, others) = (super::flock(client).await?, super::tables(client).await?);
        let root = &settings.git.checkout;
        share_none(
            &rows,
            &others,
            name.as_str(),
            place.home,
            root,
            remote.as_str(),
        )
        .map_err(|taken| taken.to_string())?;
    }
    let request = Request::SetSheepDogSettings {
        name: name.as_str().to_owned(),
        dog: DOG.to_owned(),
        table: changed.project.map(DogTable::from),
    };
    send(client, request, |r| {
        matches!(r, Response::SheepDogSettingsSet { .. })
    })
    .await?;
    let reaches = match waits_for_start(key) {
        true => "a running runner takes it when it next starts",
        false => "a running runner takes it at its next wake",
    };
    Ok(vec![format!("project {name}: {key} = {shown}; {reaches}")])
}

// Whether a runner keeps the value it started with: its reread leaves these until a start.
fn waits_for_start(key: &str) -> bool {
    matches!(key, "git" | "git.checkout" | "git.remote" | "hooks") || key.starts_with("hooks.")
}

// The settings in `tables`, when they pass the checks a runner makes of them as it
// starts and rereads that need only the files: its agents, reviewers, rulings,
// gateways, skills and instructions.
fn runnable(tables: &Tables, name: &ProjectName, place: Place<'_>) -> Result<Settings, String> {
    let paths = place.paths;
    let invalid = |e: SettingsError| e.to_string();
    let loaded = source::load(tables, name.as_str(), place.home, &paths.folder).map_err(invalid)?;
    let settings = loaded.settings;
    let book = Agents::load(&paths.agents).map_err(|e| e.to_string())?;
    let agents = settings.role_agents(&book).map_err(invalid)?;
    let lineup = settings.lineup(&book, place.home).map_err(invalid)?;
    if let Some(reason) = settings.rulings.refusal() {
        let setting = "rulings.pm_alone";
        return Err(invalid(SettingsError::Invalid { setting, reason }));
    }
    check_gateways(&loaded.kelpie.gateways(), &book, &agents, &lineup).map_err(invalid)?;
    crate::skills::check(&settings.skills, &paths.skills).map_err(invalid)?;
    crate::runner::check_instructions(&settings).map_err(invalid)?;
    Ok(settings)
}

async fn table_of_project(
    client: &dyn Connection,
    name: &ProjectName,
) -> Result<Map<String, Value>, String> {
    let tables = shepherd::read_tables_with(client, name.as_str()).await?;
    tables.project.ok_or_else(|| no_table(name))
}

fn no_table(name: &ProjectName) -> String {
    format!("project {name} has no settings: `shep kelpie add {name}` registers it")
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
