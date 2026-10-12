//! The plugins a repo turns on
//!
//! A repo's `.claude/settings.json` names a marketplace in
//! `extraKnownMarketplaces` and the plugin in `enabledPlugins`. Claude Code
//! loads only a plugin that is installed, and a headless run installs none,
//! so kelpie does it: it reads that file from the checkout's default branch
//! (never a worker's branch, so a branch cannot turn a plugin on for itself),
//! finds each enabled plugin's folder in its marketplace, and copies the
//! plugin's skills into a plugin of the same name under the project's skills
//! folder. Every call the runner makes gets that folder with `--plugin-dir`.
//!
//! Only a plugin's skills load so far. A plugin that cannot load is skipped
//! with its reason, and the run goes on.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use super::is_kelpies_own;
use crate::ports::{Git, GitError};
use crate::settings::SkillName;

/// The folder under the skills folder that holds the repo's plugins, so none
/// is ever kelpie's own
const FOLDER: &str = "repo";

/// Where the marketplaces kelpie clones are kept, under kelpie's home
pub const MARKETPLACES: &str = "marketplaces";

const SETTINGS: &str = ".claude/settings.json";

/// The manifest keys a copied plugin keeps: what names and describes it. The
/// rest of a manifest points at parts kelpie does not copy yet.
const MANIFEST_KEYS: [&str; 6] = [
    "name",
    "version",
    "description",
    "author",
    "license",
    "homepage",
];

/// One plugin the repo turns on, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoPlugin {
    /// The plugin as the repo's settings name it, `name@marketplace`
    pub plugin: String,
    /// The skills it brought, where it loaded
    pub skills: Vec<String>,
    /// Why it was left out, where it was
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// Why it is an older copy, where its marketplace could not be fetched
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
    #[serde(skip)]
    dir: Option<PathBuf>,
    // Where it is read from, until `install` has copied it to `dir`.
    #[serde(skip)]
    found: Option<Found>,
}

/// The plugins the repo turns on, as the runner loaded them
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoPlugins {
    plugins: Vec<RepoPlugin>,
}

impl RepoPlugins {
    /// Loads the plugins the checkout's default branch turns on, writing each
    /// under `skills/repo/<name>`
    ///
    /// Nothing here fails: a plugin that cannot load is skipped.
    pub fn load(checkout: &Path, kelpie_home: &Path, skills: &Path, git: &dyn Git) -> Self {
        Self::plan(checkout, kelpie_home, skills, git).install(skills)
    }

    /// What [`Self::load`] would load, with the marketplaces fetched and
    /// nothing written under `skills`
    ///
    /// A reread plans first, so a reread that fails later leaves the folders
    /// the runner is using as they were.
    pub fn plan(checkout: &Path, kelpie_home: &Path, skills: &Path, git: &dyn Git) -> Self {
        let into = skills.join(FOLDER);
        let plugins = resolve(checkout, kelpie_home, git, true)
            .into_iter()
            .map(|resolved| plan_one(resolved, &into))
            .collect();
        Self { plugins }
    }

    /// Writes each planned plugin under `skills/repo/<name>`, and takes out
    /// what the repo no longer turns on, so no call keeps it
    #[must_use]
    pub fn install(self, skills: &Path) -> Self {
        let into = skills.join(FOLDER);
        let plugins: Vec<RepoPlugin> = (self.plugins.into_iter())
            .map(|mut plugin| {
                let failed = match (&plugin.found, &plugin.dir) {
                    (Some(found), Some(dir)) => install(found, dir).err(),
                    _ => None,
                };
                if let Some(reason) = failed {
                    plugin = RepoPlugin::skipped(plugin.plugin, reason);
                }
                plugin
            })
            .collect();
        let loaded: Vec<&PathBuf> = plugins.iter().filter_map(|p| p.dir.as_ref()).collect();
        if let Ok(entries) = fs::read_dir(&into) {
            for entry in entries.filter_map(Result::ok) {
                if !loaded.contains(&&entry.path()) {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Self { plugins }
    }

    /// Every plugin folder that loaded, for `claude --plugin-dir`
    pub fn dirs(&self) -> impl Iterator<Item = &Path> {
        self.plugins.iter().filter_map(|p| p.dir.as_deref())
    }

    /// Each plugin the repo turns on, with its skills or why it was skipped
    pub fn status(&self) -> &[RepoPlugin] {
        &self.plugins
    }

    /// A log line for each plugin that was skipped or is an older copy
    pub fn notices(&self) -> impl Iterator<Item = String> + '_ {
        self.plugins
            .iter()
            .filter_map(|p| match (&p.skipped, &p.stale) {
                (Some(reason), _) => Some(format!(
                    "the repo's plugin {} is skipped: {reason}",
                    p.plugin
                )),
                (None, Some(reason)) => Some(format!(
                    "the repo's plugin {} is the copy from the last good fetch: {reason}",
                    p.plugin
                )),
                (None, None) => None,
            })
    }
}

impl RepoPlugin {
    fn skipped(plugin: String, reason: String) -> Self {
        Self {
            plugin,
            skills: Vec::new(),
            skipped: Some(reason),
            stale: None,
            dir: None,
            found: None,
        }
    }
}

// A plugin that was found, read for its skills and aimed at its folder under
// `into`, or the reason it cannot load.
fn plan_one(resolved: Result<Found, Skipped>, into: &Path) -> RepoPlugin {
    let found = match resolved {
        Ok(found) => found,
        Err(skipped) => return RepoPlugin::skipped(skipped.id, skipped.reason),
    };
    match skills_in(&found.folder) {
        Ok(skills) => RepoPlugin {
            plugin: found.id.clone(),
            skills,
            skipped: None,
            stale: found.stale.clone(),
            dir: Some(into.join(&found.name)),
            found: Some(found),
        },
        Err(reason) => RepoPlugin::skipped(found.id, reason),
    }
}

/// A plugin the repo turns on whose folder was found
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    /// `name@marketplace`
    pub(crate) id: String,
    /// The plugin's name
    pub(crate) name: String,
    /// Its folder in the marketplace
    pub(crate) folder: PathBuf,
    /// Why the marketplace is the last good copy, where it could not be fetched
    pub(crate) stale: Option<String>,
}

/// A plugin the repo turns on that cannot load
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skipped {
    /// `name@marketplace`, or what was read in its place
    pub(crate) id: String,
    pub(crate) reason: String,
}

/// Finds the folder of each plugin the checkout's default branch turns on,
/// fetching the marketplaces that need it, in the order of the repo's list
/// and `origin`'s default branch first where `refresh` is set
pub(crate) fn resolve(
    checkout: &Path,
    kelpie_home: &Path,
    git: &dyn Git,
    refresh: bool,
) -> Vec<Result<Found, Skipped>> {
    let settings = match settings_on_default_branch(checkout, refresh) {
        Ok(Some(settings)) => settings,
        Ok(None) => return Vec::new(),
        Err(reason) => {
            let id = SETTINGS.to_owned();
            return vec![Err(Skipped { id, reason })];
        }
    };
    let enabled = settings["enabledPlugins"].as_object();
    let known = settings["extraKnownMarketplaces"].as_object();
    let mut markets: BTreeMap<String, Result<(PathBuf, Option<String>), String>> = BTreeMap::new();
    let mut supplied: BTreeMap<String, String> = BTreeMap::new();
    let mut resolved = Vec::new();
    for (id, on) in enabled.into_iter().flatten() {
        if on != &Value::Bool(true) {
            continue;
        }
        let skip = |reason: String| Skipped {
            id: id.clone(),
            reason,
        };
        let Some((name, market)) = id.split_once('@') else {
            resolved.push(Err(skip("it is not written name@marketplace".into())));
            continue;
        };
        if !plain(name) || is_kelpies_own(name) {
            let what = if plain(name) {
                "which is kelpie's own"
            } else {
                "which is not a plugin's name"
            };
            resolved.push(Err(skip(format!("it is named {name}, {what}"))));
            continue;
        }
        let root = markets
            .entry(market.to_owned())
            .or_insert_with(|| marketplace(market, known, checkout, kelpie_home, git));
        let found = match root {
            Ok((root, stale)) => plugin_in(root, name).map(|folder| Found {
                id: id.clone(),
                name: name.to_owned(),
                folder,
                stale: stale.clone(),
            }),
            Err(reason) => Err(reason.clone()),
        };
        // Two plugins of one name would share a folder, the last overwriting the first.
        let found = found.and_then(|found| match supplied.get(name) {
            Some(first) => Err(format!("{first} already supplies a plugin named {name}")),
            None => {
                supplied.insert(name.to_owned(), id.clone());
                Ok(found)
            }
        });
        resolved.push(found.map_err(skip));
    }
    resolved
}

// `.claude/settings.json` as the default branch has it, read from the
// checkout's `origin` so a worker's branch never reaches it.
fn settings_on_default_branch(checkout: &Path, refresh: bool) -> Result<Option<Value>, String> {
    use crate::worktree::{BASE, git};
    // The checkout may be behind a merge that turned a plugin on; failing
    // to fetch leaves what it has.
    if refresh {
        let _ = git(checkout, ["fetch", "--quiet", "origin", BASE]);
    }
    let spec = format!("origin/{BASE}:{SETTINGS}");
    if git(checkout, ["cat-file", "-e", &spec]).is_err() {
        return Ok(None);
    }
    let text = git(checkout, ["show", &spec]).map_err(|e| e.to_string())?;
    match serde_json::from_str::<Value>(&text) {
        Ok(settings) if settings.is_object() => Ok(Some(settings)),
        Ok(_) => Err(format!("{SETTINGS} on {BASE} is not a JSON object")),
        Err(e) => Err(format!("{SETTINGS} on {BASE} is not valid JSON: {e}")),
    }
}

// Where marketplace `name`'s repo is on disk, fetched first where it is a
// git repo, and why the copy is old if the fetch failed.
fn marketplace(
    name: &str,
    known: Option<&Map<String, Value>>,
    checkout: &Path,
    kelpie_home: &Path,
    git: &dyn Git,
) -> Result<(PathBuf, Option<String>), String> {
    let entry = known
        .and_then(|known| known.get(name))
        .ok_or_else(|| format!("{SETTINGS} has no extraKnownMarketplaces entry for {name}"))?;
    let source = &entry["source"];
    let text = |key: &str| source[key].as_str().filter(|s| !s.is_empty());
    let (url, directory) = match source["source"].as_str() {
        Some("github") => {
            let repo = text("repo").ok_or("its github source names no repo")?;
            (Some(format!("https://github.com/{repo}.git")), None)
        }
        Some("git") => (
            Some(text("url").ok_or("its git source has no url")?.to_owned()),
            None,
        ),
        Some("directory") => (
            None,
            Some(text("path").ok_or("its directory source has no path")?),
        ),
        Some(other) => return Err(format!("its {other} source is not one kelpie loads")),
        None => return Err(format!("the {name} marketplace names no source")),
    };
    if let Some(path) = directory {
        return inside(checkout, Path::new(path))
            .map(|folder| (folder, None))
            .map_err(|()| format!("its directory {path} is not a folder inside the checkout"));
    }
    let url = url.unwrap_or_default();
    // A leading `-` would be taken for an option, and `::` names a transport
    // that runs a program.
    if url.starts_with('-') || url.contains("::") {
        return Err(format!("{url} is not a repo address"));
    }
    if !plain(name) {
        return Err(format!(
            "{name} is not a name kelpie keeps a marketplace under"
        ));
    }
    let into = kelpie_home.join(MARKETPLACES).join(name);
    let reference = text("ref");
    match git.sync(&url, reference, &into) {
        Ok(()) => Ok((into, None)),
        Err(e) if into.join(".claude-plugin/marketplace.json").is_file() => Ok((
            into,
            Some(format!("fetching the {name} marketplace failed: {e}")),
        )),
        Err(e) => Err(format!("fetching the {name} marketplace failed: {e}")),
    }
}

// The folder of plugin `name` in the marketplace at `root`.
fn plugin_in(root: &Path, name: &str) -> Result<PathBuf, String> {
    let file = root.join(".claude-plugin/marketplace.json");
    let listed: Value = fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("{} is not a readable marketplace", file.display()))?;
    let entry = listed["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["name"].as_str() == Some(name))
        .ok_or_else(|| format!("the marketplace lists no plugin {name}"))?;
    let Some(source) = entry["source"].as_str() else {
        return Err("it comes from outside its marketplace, which kelpie does not load yet".into());
    };
    inside(root, Path::new(source))
        .map_err(|()| format!("its source {source} leaves the marketplace"))
}

// `relative` under `root`, if it names a place there: no `..`, no absolute
// path, and no link that leads out. A place that does not exist yet is
// returned for the caller to find missing.
fn inside(root: &Path, relative: &Path) -> Result<PathBuf, ()> {
    let plain = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !plain {
        return Err(());
    }
    let place = root.join(relative);
    match (place.canonicalize(), root.canonicalize()) {
        (Ok(place_now), Ok(root_now)) if !place_now.starts_with(&root_now) => Err(()),
        _ => Ok(place),
    }
}

// The names of the skills in the plugin at `folder`.
fn skills_in(folder: &Path) -> Result<Vec<String>, String> {
    let root = folder.join("skills");
    if !is_folder(&root) {
        return Err(format!("{} has no skills folder", folder.display()));
    }
    // Only what `copy_plain` copies counts.
    let mut skills: Vec<String> = fs::read_dir(root)
        .map_err(|_| format!("{} has no skills folder", folder.display()))?
        .filter_map(Result::ok)
        .filter(|entry| is_folder(&entry.path()) && is_file(&entry.path().join("SKILL.md")))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    skills.sort();
    if skills.is_empty() {
        return Err("it has no skills, and only a plugin's skills load so far".into());
    }
    Ok(skills)
}

/// What a runner starting now would load of the checkout's plugins, from the
/// marketplaces already fetched, writing and fetching nothing of kelpie's
pub fn inspect(checkout: &Path, kelpie_home: &Path) -> Vec<RepoPlugin> {
    resolve(checkout, kelpie_home, &Fetched, false)
        .into_iter()
        .map(|resolved| plan_one(resolved, Path::new("")))
        .collect()
}

// A git that fetches nothing: a marketplace is as the last runner left it.
struct Fetched;

impl Git for Fetched {
    fn sync(&self, _: &str, _: Option<&str>, into: &Path) -> Result<(), GitError> {
        if into.join(".claude-plugin/marketplace.json").is_file() {
            return Ok(());
        }
        Err(GitError(
            "it is not fetched yet, which a runner does as it starts".into(),
        ))
    }
}

// Writes the plugin's skills and a trimmed manifest into `dir`, replacing
// what was there.
fn install(found: &Found, dir: &Path) -> Result<(), String> {
    let from = found.folder.join("skills");
    let manifest = manifest(&found.folder, &found.name);
    let written = (|| {
        match fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        fs::create_dir_all(dir.join(".claude-plugin"))?;
        fs::write(dir.join(".claude-plugin/plugin.json"), manifest)?;
        copy_plain(&from, &dir.join("skills"))
    })();
    written.map_err(|e| format!("cannot copy it into {}: {e}", dir.display()))
}

// Copies `from` into `to`, files and folders only. A link is left out, since
// a marketplace is not kelpie's and one could point at any file kelpie can read.
fn copy_plain(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_plain(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// Whether `path` is a folder itself and no link to one.
fn is_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

// Whether `path` is a file itself and no link to one.
fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

// The plugin's manifest with its name forced and its parts left out, or a
// bare one where the plugin has none.
fn manifest(plugin: &Path, name: &str) -> String {
    let file = plugin.join(".claude-plugin/plugin.json");
    let read: Value = fs::read_to_string(&file)
        .ok()
        .filter(|_| is_file(&file))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let mut kept = Map::new();
    for key in MANIFEST_KEYS {
        if let Some(value) = read.get(key) {
            kept.insert(key.to_owned(), value.clone());
        }
    }
    kept.insert("name".to_owned(), Value::String(name.to_owned()));
    Value::Object(kept).to_string()
}

// Whether `name` is one plain path component.
fn plain(name: &str) -> bool {
    SkillName::try_from(name.to_owned()).is_ok()
}

#[cfg(test)]
mod tests;
