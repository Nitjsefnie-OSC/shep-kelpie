//! Finding a plugin in its marketplace and copying the parts that load

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{Installed, inside, refused};
use crate::ports::McpServer;
use crate::settings::{PluginId, PluginPart};

const PLUGIN_ROOT: &str = "${CLAUDE_PLUGIN_ROOT}";

/// A plugin found in its marketplace
pub(super) struct Resolved {
    folder: PathBuf,
    pub name: String,
    manifest: Value,
    /// The parts it has
    pub present: Vec<PluginPart>,
}

// A plugin's manifest key for each part, and its default place in the folder.
fn key(part: PluginPart) -> (&'static str, &'static str) {
    match part {
        PluginPart::Skills => ("skills", "skills"),
        PluginPart::Agents => ("agents", "agents"),
        PluginPart::Commands => ("commands", "commands"),
        PluginPart::Hooks => ("hooks", "hooks"),
        PluginPart::Mcp => ("mcpServers", ".mcp.json"),
    }
}

/// Finds `id` in the marketplace at `market` and reads what it has
///
/// # Errors
///
/// Why the plugin cannot load: the marketplace's file is unreadable, it lists
/// no such plugin or one outside its own folder, or the plugin's name is
/// kelpie's.
pub(super) fn resolve(market: &Path, id: &PluginId) -> Result<Resolved, String> {
    if refused(id.name()) {
        return Err(refusal(id.name()));
    }
    let file = market.join(".claude-plugin/marketplace.json");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {}", file.display(), e.kind()))?;
    let listing: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not readable JSON: {e}", file.display()))?;
    let entry = listing["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["name"] == id.name())
        .ok_or_else(|| {
            format!(
                "marketplace {} lists no plugin {}",
                id.marketplace(),
                id.name()
            )
        })?;
    let Some(source) = entry["source"].as_str() else {
        return Err(
            "its marketplace entry's source is not a folder of the marketplace, which is all \
             kelpie loads"
                .to_owned(),
        );
    };
    let root = listing["metadata"]["pluginRoot"].as_str().unwrap_or("");
    if !inside(source) || !inside(root) {
        return Err(format!(
            "its source {source} leaves the marketplace's own folder"
        ));
    }
    let folder = market.join(root).join(source);
    if !folder.is_dir() {
        return Err(format!("its folder {} is not there", folder.display()));
    }
    let manifest_file = folder.join(".claude-plugin/plugin.json");
    let mut manifest = match std::fs::read_to_string(&manifest_file) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("{} is not a readable manifest", manifest_file.display()))?,
        Err(_) => Value::Object(Map::new()),
    };
    // A marketplace entry can carry what a manifest would.
    for part in PluginPart::all() {
        let (name, _) = key(part);
        if manifest.get(name).is_none()
            && let Some(value) = entry.get(name)
        {
            manifest[name] = value.clone();
        }
    }
    let name = manifest["name"].as_str().unwrap_or(id.name()).to_owned();
    if refused(&name) {
        return Err(refusal(&name));
    }
    if !crate::settings::plain(&name) {
        return Err(format!("its name {name:?} is not a plain name"));
    }
    let present = PluginPart::all()
        .into_iter()
        .filter(|part| {
            let (name, place) = key(*part);
            manifest.get(name).is_some() || folder.join(place).exists()
        })
        .collect();
    Ok(Resolved {
        folder,
        name,
        manifest,
        present,
    })
}

fn refusal(name: &str) -> String {
    format!("{name} is a name kelpie keeps for its own plugins")
}

/// Copies the plugin into `into/<its name>`, with only the `parts` that load
///
/// Its MCP servers leave the copy and go to the call's MCP config instead,
/// which is how a call that allows nothing but its own list gets them. A
/// server named as one in `taken` is prefixed with the plugin's name.
///
/// # Errors
///
/// Why the copy could not be made.
pub(super) fn copy(
    found: &Resolved,
    parts: &[PluginPart],
    into: &Path,
    taken: &[&str],
) -> Result<Installed, String> {
    let dir = into.join(&found.name);
    let failed = |e: std::io::Error| {
        format!(
            "cannot copy {} into {}: {}",
            found.folder.display(),
            dir.display(),
            e.kind()
        )
    };
    crate::skills::copy_folder_without(&found.folder, &dir, ".git").map_err(failed)?;
    let mut manifest = found.manifest.clone();
    let mcp = if parts.contains(&PluginPart::Mcp) {
        servers(found, &dir, taken)
    } else {
        Vec::new()
    };
    for part in PluginPart::all() {
        let (name, place) = key(part);
        // The servers go to the call's config whatever the plugin keeps.
        if parts.contains(&part) && part != PluginPart::Mcp {
            continue;
        }
        if let Some(m) = manifest.as_object_mut() {
            m.remove(name);
        }
        let gone = dir.join(place);
        let removed = if gone.is_dir() {
            std::fs::remove_dir_all(&gone)
        } else {
            std::fs::remove_file(&gone)
        };
        match removed {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(failed(e)),
            _ => {}
        }
    }
    manifest["name"] = found.name.clone().into();
    std::fs::create_dir_all(dir.join(".claude-plugin")).map_err(failed)?;
    let text = serde_json::to_string_pretty(&manifest).expect("a manifest is JSON");
    std::fs::write(dir.join(".claude-plugin/plugin.json"), text).map_err(failed)?;
    Ok(Installed {
        dir,
        name: found.name.clone(),
        mcp,
    })
}

// The servers a plugin brings, from its manifest and its `.mcp.json`, each
// with `${CLAUDE_PLUGIN_ROOT}` made the copy's own folder, since they no
// longer start from the plugin.
fn servers(found: &Resolved, dir: &Path, taken: &[&str]) -> Vec<McpServer> {
    let mut listed: Vec<(String, Value)> = Vec::new();
    let mut from_object = |object: &Value| {
        // A file holds either the table of servers or `{ "mcpServers": table }`.
        let table = object.get("mcpServers").unwrap_or(object);
        for (name, config) in table.as_object().into_iter().flatten() {
            listed.push((name.clone(), config.clone()));
        }
    };
    match &found.manifest["mcpServers"] {
        Value::String(path) if inside(path) => {
            if let Some(read) = read_json(&found.folder.join(path)) {
                from_object(&read);
            }
        }
        object @ Value::Object(_) => from_object(object),
        _ => {}
    }
    if let Some(read) = read_json(&found.folder.join(".mcp.json")) {
        from_object(&read);
    }
    let root = dir.to_string_lossy();
    let mut kept: Vec<McpServer> = Vec::new();
    for (name, mut config) in listed {
        let mut name: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if taken.contains(&name.as_str()) || kept.iter().any(|s| s.name == name) {
            name = format!("{}_{name}", found.name.replace(['.', '-'], "_"));
        }
        substitute(&mut config, &root);
        kept.push(McpServer { name, config });
    }
    kept
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

fn substitute(value: &mut Value, root: &str) {
    match value {
        Value::String(text) => *text = text.replace(PLUGIN_ROOT, root),
        Value::Array(items) => items.iter_mut().for_each(|v| substitute(v, root)),
        Value::Object(map) => map.values_mut().for_each(|v| substitute(v, root)),
        _ => {}
    }
}
