//! `shep kelpie settings migrate`: writes the keys each project's table is missing
//!
//! A project's old `rulings.pm_alone` list becomes `rulings.pm_decides`, and
//! each kind of ruling the map lacks is written at its default. A key already
//! there is never changed. Each table is checked as `set` checks it, and one
//! the runner would refuse is not written.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Map, Value};

use super::{Homes, Place, checked, lookup, write};
use crate::home::{ProjectName, ProjectPaths};
use crate::settings::{PM_ALONE, PM_DECIDES, RulingName};
use crate::shepherd::{self, Connection, Socket};

/// Runs `shep kelpie settings migrate`, printing a line for each project it changed
pub fn main() -> ExitCode {
    let ran = crate::shep_home::required(crate::shep_home::FLOCK_FIX).and_then(|shep_home| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set")?;
        let kelpie_home = crate::home::kelpie_home_of(&shep_home);
        let homes = Homes {
            home: &home,
            kelpie_home: &kelpie_home,
            shep_home: &shep_home,
        };
        shepherd::block_on(async {
            let client = shepherd::connect(&Socket, &shep_home)
                .await
                .map_err(|e| e.describe(&shep_home))?;
            migrate(&client, homes).await
        })
    });
    match ran {
        Ok(lines) => {
            lines.iter().for_each(|line| println!("{line}"));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("shep kelpie settings migrate: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the keys every project's table is missing, a line for each project changed
///
/// `nothing to migrate` when no table misses one.
///
/// # Errors
///
/// A message when the shepherd cannot be read, or when a project's changed
/// table is refused or not written: it names each such project and why,
/// after the lines of the projects that were written.
pub async fn migrate(client: &dyn Connection, homes: Homes<'_>) -> Result<Vec<String>, String> {
    let tables = super::super::tables(client).await?;
    let mut said = Vec::new();
    let mut refused = Vec::new();
    for sheep in tables.keys() {
        let Ok(name) = ProjectName::try_from(sheep.as_str()) else {
            continue;
        };
        let paths = ProjectPaths::under(homes.kelpie_home, homes.shep_home, &name);
        let place = Place {
            home: homes.home,
            paths: &paths,
        };
        match project(client, &name, place).await {
            Ok(Some(line)) => said.push(line),
            Ok(None) => {}
            Err(why) => refused.push(format!("project {name}: not migrated: {why}")),
        }
    }
    if !refused.is_empty() {
        return Err(said
            .into_iter()
            .chain(refused)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if said.is_empty() {
        said.push("nothing to migrate".to_owned());
    }
    Ok(said)
}

// Migrates one project's table, and says what it wrote, if anything.
async fn project(
    client: &dyn Connection,
    name: &ProjectName,
    place: Place<'_>,
) -> Result<Option<String>, String> {
    let read = shepherd::read_tables_with(client, name.as_str()).await?;
    let Some(mut table) = read.project.clone() else {
        return Ok(None);
    };
    let listed = lookup(&table, PM_ALONE).is_some();
    let wrote = pm_decides(&mut table);
    if wrote.is_empty() {
        return Ok(None);
    }
    let removed = match listed && lookup(&table, PM_ALONE).is_none() {
        true => format!("; removed {PM_ALONE}"),
        false => String::new(),
    };
    let (request, _) = checked(client, name, place, read, table).await?;
    write(client, &request).await?;
    Ok(Some(format!(
        "project {name}: wrote {}{removed}",
        wrote.join(", ")
    )))
}

/// Writes into `table` the kinds `rulings.pm_decides` is missing, at their
/// defaults, after the old `rulings.pm_alone` list's kinds as `true`
///
/// Names each key it wrote. A key already there is left as it is. So is a
/// list beside a map, or one with an entry that is not a kind's name: the
/// check refuses either, naming it.
pub fn pm_decides(table: &mut Map<String, Value>) -> Vec<String> {
    let ((rulings, map_key), (_, list_key)) = (leaves(PM_DECIDES), leaves(PM_ALONE));
    let rulings = table
        .entry(rulings)
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(rulings) = rulings.as_object_mut() else {
        return Vec::new();
    };
    let names_only = |list: &Value| {
        list.as_array()
            .is_some_and(|list| list.iter().all(Value::is_string))
    };
    let moves = !rulings.contains_key(map_key) && rulings.get(list_key).is_some_and(names_only);
    let listed = match moves.then(|| rulings.remove(list_key)).flatten() {
        Some(Value::Array(list)) => list,
        _ => Vec::new(),
    };
    let map = rulings
        .entry(map_key)
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(map) = map.as_object_mut() else {
        return Vec::new();
    };
    let mut wrote = Vec::new();
    let listed = listed
        .iter()
        .filter_map(Value::as_str)
        .map(|kind| (kind, true));
    let defaults =
        (RulingName::ALL.into_iter()).filter_map(|kind| Some((kind.as_str(), kind.pm_default()?)));
    for (kind, value) in listed.chain(defaults) {
        if !map.contains_key(kind) {
            map.insert(kind.to_owned(), Value::Bool(value));
            wrote.push(format!("{PM_DECIDES}.{kind} = {value}"));
        }
    }
    wrote
}

// The table and the key in it that a dotted `rulings.<key>` setting names.
fn leaves(setting: &'static str) -> (&'static str, &'static str) {
    setting.split_once('.').unwrap_or(("", setting))
}
