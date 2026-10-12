//! The plugins a repo turns on, loaded for every agent kelpie runs there
//!
//! A repo turns a Claude Code plugin on for everyone who works in it:
//! `.claude/settings.json` names a marketplace in `extraKnownMarketplaces`
//! and the plugin in `enabledPlugins`. Claude Code only loads an enabled
//! plugin that is installed, and a headless run never installs one, so
//! kelpie does the installing. It reads the file from the checkout's default
//! branch, never from a worker's branch, so a branch cannot turn a plugin on
//! for itself, finds each enabled plugin in its marketplace, and copies it
//! under the project's skills folder as a plugin of its own name. Each call
//! of a role the project's `plugins` table lets gets it as one more
//! `--plugin-dir`.
//!
//! A plugin that cannot load is skipped and `status` says why: the run goes on.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::ports::{AgentCall, Marketplaces, McpServer, Role};
use crate::settings::{MarketplaceSource, PluginId, PluginPart, PluginRole, Plugins};

mod install;

/// Why a plugin the project names in `plugins.skip` is left out
pub const SKIPPED_BY_SETTING: &str = "it is named in plugins.skip";

/// The plugin kelpie keeps for the skills it vendors
const KELPIE_PLUGIN: &str = crate::skills::PLUGIN;

/// Where the marketplaces and the repo's file are
#[derive(Clone, Copy)]
pub struct Source<'a> {
    /// The project's checkout, whose default branch holds `.claude/settings.json`
    pub checkout: &'a Path,
    /// The checkout's default branch, where the forge said it
    pub branch: Option<&'a str>,
    /// Where a git marketplace is cloned: `<kelpie home>/marketplaces`
    pub marketplaces: &'a Path,
    /// What clones them
    pub git: &'a dyn Marketplaces,
}

impl std::fmt::Debug for Source<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("checkout", &self.checkout)
            .field("branch", &self.branch)
            .field("marketplaces", &self.marketplaces)
            .finish_non_exhaustive()
    }
}

/// One plugin, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginStatus {
    /// `name@marketplace`
    pub plugin: String,
    /// Whether the repo turns it on (`repo`) or the project asks for it (`extra`)
    pub from: &'static str,
    /// The parts of it that load, such as `skills`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub loaded: Vec<&'static str>,
    /// Why it was skipped, where it was
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Installed {
    dir: PathBuf,
    name: String,
    mcp: Vec<McpServer>,
}

/// The plugins a runner loaded, and what each call gets of them
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoPlugins {
    roles: Vec<PluginRole>,
    installed: Vec<Installed>,
    status: Vec<PluginStatus>,
    notices: Vec<String>,
    // The branch `.claude/settings.json` was read from, and the file as read,
    // to tell when it moves
    branch: Option<String>,
    read: Option<String>,
}

impl RepoPlugins {
    /// Loads every plugin the repo turns on and `plugins` asks for
    ///
    /// With `into`, each is copied there, in a folder of its name, and that
    /// folder is emptied first. Without, nothing is written and `status`
    /// says what would load, which is what `doctor` wants. Nothing here
    /// fails: a plugin that cannot load is skipped.
    pub fn load(plugins: &Plugins, from: &Source<'_>, into: Option<&Path>) -> Self {
        let mut loaded = Self {
            roles: plugins.roles.clone(),
            ..Self::default()
        };
        if let Some(into) = into {
            match std::fs::remove_dir_all(into) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    loaded.notices.push(format!(
                        "cannot clear {} for the repo's plugins: {}",
                        into.display(),
                        e.kind()
                    ));
                }
                _ => {}
            }
            if let Err(e) = std::fs::create_dir_all(into) {
                loaded.notices.push(format!(
                    "cannot make {} for the repo's plugins: {}",
                    into.display(),
                    e.kind()
                ));
                return loaded;
            }
        }
        if !plugins.repo && plugins.extra.is_empty() {
            return loaded;
        }
        let repo = loaded.read_repo(from, into.is_some());
        let mut wanted: Vec<(PluginId, &'static str)> = Vec::new();
        if plugins.repo {
            let enabled = repo["enabledPlugins"].as_object().into_iter().flatten();
            for (key, _) in enabled.filter(|(_, on)| on.as_bool() == Some(true)) {
                match PluginId::parse(key) {
                    Some(id) => wanted.push((id, "repo")),
                    None => loaded.status.push(PluginStatus {
                        plugin: key.clone(),
                        from: "repo",
                        loaded: Vec::new(),
                        skipped: Some("it is not named name@marketplace".to_owned()),
                    }),
                }
            }
        }
        for id in &plugins.extra {
            if !wanted.iter().any(|(w, _)| w == id) {
                wanted.push((id.clone(), "extra"));
            }
        }
        let mut markets = Markets::default();
        for (id, origin) in wanted {
            let outcome = if plugins.skip.contains(&id) {
                Err(SKIPPED_BY_SETTING.to_owned())
            } else {
                loaded.one(&id, plugins, (&repo, from, &mut markets), into)
            };
            let (parts, skipped) = match outcome {
                Ok(parts) => (parts, None),
                Err(reason) => (Vec::new(), Some(reason)),
            };
            loaded.status.push(PluginStatus {
                plugin: id.to_string(),
                from: origin,
                loaded: parts,
                skipped,
            });
        }
        loaded.notices.extend(markets.notices);
        loaded
    }

    // The repo's `.claude/settings.json` from its default branch, empty where
    // there is none or it cannot be read.
    fn read_repo(&mut self, from: &Source<'_>, fetch: bool) -> Value {
        let Some(branch) = from.branch else {
            self.notices.push(
                "the repo's default branch could not be read, so the plugins its \
                 .claude/settings.json turns on are not loaded"
                    .to_owned(),
            );
            return Value::Null;
        };
        self.branch = Some(branch.to_owned());
        // A dry load reads what the checkout has, and fetches nothing.
        self.read = repo_settings(from.checkout, branch, fetch);
        match self.read.as_deref().map(serde_json::from_str::<Value>) {
            Some(Ok(value)) => value,
            Some(Err(e)) => {
                self.notices.push(format!(
                    ".claude/settings.json on {branch} is not readable JSON, so it turns on no plugin: {e}"
                ));
                Value::Null
            }
            None => Value::Null,
        }
    }

    // Finds `id` in its marketplace and copies it, and returns the parts that load.
    fn one(
        &mut self,
        id: &PluginId,
        plugins: &Plugins,
        (repo, from, markets): (&Value, &Source<'_>, &mut Markets),
        into: Option<&Path>,
    ) -> Result<Vec<&'static str>, String> {
        let source = marketplace_source(id.marketplace(), plugins, repo, from.checkout)?;
        let folder = markets.folder(id.marketplace(), &source, from)?;
        let found = install::resolve(&folder, id)?;
        if self.installed.iter().any(|i| i.name == found.name) {
            return Err(format!(
                "another plugin named {} is already loaded",
                found.name
            ));
        }
        let parts: Vec<PluginPart> = found
            .present
            .iter()
            .copied()
            .filter(|part| plugins.parts.contains(part))
            .collect();
        if parts.is_empty() {
            return Err(format!(
                "it has none of the parts plugins.parts loads ({})",
                names(&plugins.parts)
            ));
        }
        if let Some(into) = into {
            let taken: Vec<&str> = (self.installed.iter())
                .flat_map(|i| i.mcp.iter().map(|m| m.name.as_str()))
                .collect();
            let installed = install::copy(&found, &parts, into, &taken)?;
            self.installed.push(installed);
        }
        Ok(parts.into_iter().map(PluginPart::as_str).collect())
    }

    /// Adds the plugins to `call` if its role gets them: a `--plugin-dir`
    /// for each, and the MCP servers they bring
    pub fn apply(&self, call: &mut AgentCall) {
        let role = match call.role {
            Role::Worker => PluginRole::Worker,
            Role::Reviewer => PluginRole::Reviewer,
            Role::Pm => PluginRole::Pm,
            Role::IssueWriter | Role::Advisor | Role::Researcher => return,
        };
        if !self.roles.contains(&role) {
            return;
        }
        for installed in &self.installed {
            if !call.plugin_dirs.contains(&installed.dir) {
                call.plugin_dirs.push(installed.dir.clone());
            }
            for server in &installed.mcp {
                if !call.mcp_servers.iter().any(|s| s.name == server.name) {
                    call.mcp_servers.push(server.clone());
                }
            }
        }
    }

    /// Whether `other` has the same plugins loaded for the same roles
    pub fn loads_like(&self, other: &Self) -> bool {
        (&self.roles, &self.installed, &self.status)
            == (&other.roles, &other.installed, &other.status)
    }

    /// Each plugin and what it loaded or why it was skipped
    pub fn status(&self) -> &[PluginStatus] {
        &self.status
    }

    /// What went wrong beside a single plugin: the repo's file, a marketplace
    pub fn problems(&self) -> &[String] {
        &self.notices
    }

    /// A log line for each plugin that could not load, and each problem
    /// with the repo's file or a marketplace
    pub fn notices(&self) -> impl Iterator<Item = String> + '_ {
        let skipped = self.status.iter().filter_map(|s| {
            let reason = s.skipped.as_ref()?;
            Some(format!("the plugin {} is skipped: {reason}", s.plugin))
        });
        self.notices.iter().cloned().chain(skipped)
    }

    /// Whether the repo's `.claude/settings.json` on its default branch now
    /// differs from the one this was loaded from
    pub fn moved(&self, checkout: &Path) -> bool {
        (self.branch.as_deref())
            .is_some_and(|branch| repo_settings(checkout, branch, true) != self.read)
    }
}

fn names(parts: &[PluginPart]) -> String {
    let names: Vec<&str> = parts.iter().map(|p| p.as_str()).collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

// The file on the branch `origin` has, or the checkout's own, where there is one.
fn repo_settings(checkout: &Path, branch: &str, fetch: bool) -> Option<String> {
    if fetch {
        let _ = crate::worktree::git(checkout, ["fetch", "--quiet", "origin", branch]);
    }
    [format!("origin/{branch}"), branch.to_owned()]
        .into_iter()
        .find_map(|reference| {
            let file = format!("{reference}:.claude/settings.json");
            crate::worktree::git(checkout, ["show", file.as_str()]).ok()
        })
}

// Where `marketplace` is: the project's own entry first, then the repo's.
fn marketplace_source(
    marketplace: &str,
    plugins: &Plugins,
    repo: &Value,
    checkout: &Path,
) -> Result<MarketplaceSource, String> {
    if let Some(source) = plugins.marketplaces.get(marketplace) {
        return Ok(source.clone());
    }
    let entry = &repo["extraKnownMarketplaces"][marketplace]["source"];
    let text = |key: &str| entry[key].as_str().map(str::to_owned);
    let unnamed = || {
        format!(
            "no marketplace {marketplace}: neither the repo's extraKnownMarketplaces nor \
             plugins.marketplaces names it"
        )
    };
    match entry["source"].as_str() {
        Some("github") => text("repo").map(|repo| MarketplaceSource::Github { repo }),
        Some("git") => text("url").map(|url| MarketplaceSource::Git { url }),
        Some("directory") => text("path").map(|path| {
            let path = PathBuf::from(path);
            let path = if path.is_relative() {
                checkout.join(path)
            } else {
                path
            };
            MarketplaceSource::Directory { path }
        }),
        Some(other) => {
            return Err(format!(
                "marketplace {marketplace} has a {other} source, which kelpie cannot load: \
                 only github, git and directory"
            ));
        }
        None => None,
    }
    .ok_or_else(unnamed)
}

// Each marketplace is fetched once a load, however many plugins it holds.
#[derive(Default)]
struct Markets {
    found: BTreeMap<String, Result<PathBuf, String>>,
    notices: Vec<String>,
}

impl Markets {
    // The folder holding `name`'s `.claude-plugin/marketplace.json`.
    fn folder(
        &mut self,
        name: &str,
        source: &MarketplaceSource,
        from: &Source<'_>,
    ) -> Result<PathBuf, String> {
        if let Some(found) = self.found.get(name) {
            return found.clone();
        }
        let found = self.get(name, source, from);
        self.found.insert(name.to_owned(), found.clone());
        found
    }

    fn get(
        &mut self,
        name: &str,
        source: &MarketplaceSource,
        from: &Source<'_>,
    ) -> Result<PathBuf, String> {
        let (url, folder) = match source {
            MarketplaceSource::Directory { path } => {
                return if path.join(".claude-plugin/marketplace.json").is_file() {
                    Ok(path.clone())
                } else {
                    Err(format!(
                        "{} holds no .claude-plugin/marketplace.json",
                        path.display()
                    ))
                };
            }
            MarketplaceSource::Github { repo } => {
                let plain = repo.split_once('/').is_some_and(|(owner, name)| {
                    crate::settings::plain(owner) && crate::settings::plain(name)
                });
                if !plain {
                    return Err(format!("{repo:?} is not a GitHub repo, owner/name"));
                }
                (
                    format!("https://github.com/{repo}.git"),
                    from.marketplaces.join(name),
                )
            }
            MarketplaceSource::Git { url } => (url.clone(), from.marketplaces.join(name)),
        };
        if !crate::settings::plain(name) {
            return Err(format!("{name:?} is not a name for a marketplace"));
        }
        if let Err(e) = from.git.fetch(&url, &folder) {
            if !folder.join(".claude-plugin/marketplace.json").is_file() {
                return Err(format!("cannot fetch marketplace {name} from {url}: {e}"));
            }
            self.notices.push(format!(
                "cannot fetch marketplace {name} from {url}, so its plugins come from the \
                 last good fetch: {e}"
            ));
        }
        if folder.join(".claude-plugin/marketplace.json").is_file() {
            Ok(folder)
        } else {
            Err(format!(
                "marketplace {name} from {url} holds no .claude-plugin/marketplace.json"
            ))
        }
    }
}

// Whether kelpie keeps `name` for its own plugins: two of one name would
// leave Claude Code to pick which skill runs.
fn refused(name: &str) -> bool {
    name == KELPIE_PLUGIN || name.starts_with("kelpie-")
}

// A relative path with nothing in it that leaves the folder it is joined to.
fn inside(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests;
