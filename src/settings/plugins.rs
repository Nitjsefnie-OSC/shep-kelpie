//! The plugins every agent loads, `[app.dogs.kelpie.plugins]`
//!
//! A repo turns a Claude Code plugin on for everyone who works in it in
//! `.claude/settings.json`, and kelpie does the installing a headless run
//! skips. This table narrows what it loads. `crate::plugins` loads them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

/// Which plugins load, which of their parts, and for which calls
///
/// Left out, a repo's plugins load whole in every worker, reviewer and
/// project manager call, the way Claude Code loads them for someone working
/// in the repo.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plugins {
    /// Whether the plugins the repo turns on load, `true` by default. `false`
    /// loads none of them; `extra` still loads.
    #[serde(default = "yes")]
    pub repo: bool,
    /// Which parts of a plugin load: `skills`, `agents`, `commands`, `hooks`
    /// and `mcp`. All of them by default.
    #[serde(default = "PluginPart::all")]
    pub parts: Vec<PluginPart>,
    /// Which calls get the plugins: `worker`, `reviewer` and `pm`. All of
    /// them by default.
    #[serde(default = "PluginRole::all")]
    pub roles: Vec<PluginRole>,
    /// Plugins, as `name@marketplace`, left out though the repo turns them on
    #[serde(default)]
    pub skip: Vec<PluginId>,
    /// Plugins the repo does not turn on, as `name@marketplace`. Their
    /// marketplace is the repo's `extraKnownMarketplaces` entry of that name,
    /// or one in `marketplaces`.
    #[serde(default)]
    pub extra: Vec<PluginId>,
    /// Marketplaces for `extra`, by name, that the repo does not name
    #[serde(default)]
    pub marketplaces: BTreeMap<String, MarketplaceSource>,
}

fn yes() -> bool {
    true
}

impl Default for Plugins {
    fn default() -> Self {
        Self {
            repo: true,
            parts: PluginPart::all(),
            roles: PluginRole::all(),
            skip: Vec::new(),
            extra: Vec::new(),
            marketplaces: BTreeMap::new(),
        }
    }
}

impl Plugins {
    // Every folder the table names, which expand `~/` and are taken from
    // the settings file's folder.
    pub(super) fn paths_mut(&mut self) -> impl Iterator<Item = &mut PathBuf> {
        self.marketplaces.values_mut().filter_map(|m| match m {
            MarketplaceSource::Directory { path } => Some(path),
            MarketplaceSource::Github { .. } | MarketplaceSource::Git { .. } => None,
        })
    }
}

/// One part of a plugin
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PluginPart {
    /// Its `skills` folder
    Skills,
    /// Its `agents` folder
    Agents,
    /// Its `commands` folder
    Commands,
    /// Its hooks, which run beside kelpie's own and cannot replace them
    Hooks,
    /// Its MCP servers, added to the call's MCP config with their tools
    Mcp,
}

impl PluginPart {
    /// Every part, in the order the table lists them
    pub fn all() -> Vec<Self> {
        vec![
            Self::Skills,
            Self::Agents,
            Self::Commands,
            Self::Hooks,
            Self::Mcp,
        ]
    }

    /// The part's name in the table
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skills => "skills",
            Self::Agents => "agents",
            Self::Commands => "commands",
            Self::Hooks => "hooks",
            Self::Mcp => "mcp",
        }
    }
}

/// A kind of call that may get the plugins
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PluginRole {
    /// A worker's turn, a retro included
    Worker,
    /// A reviewer's session
    Reviewer,
    /// The project manager's session
    Pm,
}

impl PluginRole {
    /// Every role, in the order the table lists them
    pub fn all() -> Vec<Self> {
        vec![Self::Worker, Self::Reviewer, Self::Pm]
    }
}

/// A plugin as `name@marketplace`: each side letters, digits, `-`, `_` and
/// `.`, not starting with `.`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the shape goes here.
#[schemars(extend("pattern" = "^[A-Za-z0-9_-][A-Za-z0-9_.-]*@[A-Za-z0-9_-][A-Za-z0-9_.-]*$"))]
pub struct PluginId {
    name: String,
    marketplace: String,
}

impl PluginId {
    /// The plugin's name
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The marketplace it comes from
    #[inline]
    pub fn marketplace(&self) -> &str {
        &self.marketplace
    }

    /// `name@marketplace` from `text`, or `None` when either side is not a
    /// plain name
    pub fn parse(text: &str) -> Option<Self> {
        let (name, marketplace) = text.split_once('@')?;
        (plain(name) && plain(marketplace)).then(|| Self {
            name: name.to_owned(),
            marketplace: marketplace.to_owned(),
        })
    }
}

/// Whether `name` is safe to use as a folder's name and has no `@`
pub fn plain(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    !name.is_empty() && !name.starts_with('.') && name.chars().all(allowed)
}

impl TryFrom<String> for PluginId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or(
            "must be name@marketplace: letters, digits, - _ . on each side, not starting with .",
        )
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.marketplace)
    }
}

/// Where a marketplace is, as Claude Code's `extraKnownMarketplaces` says it
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "source", rename_all = "lowercase", deny_unknown_fields)]
pub enum MarketplaceSource {
    /// A GitHub repo, cloned into kelpie's home
    Github {
        /// `owner/name`
        repo: String,
    },
    /// Any git URL, cloned into kelpie's home
    Git {
        /// The URL
        url: String,
    },
    /// A folder on this machine, read where it is. A leading `~/` is the
    /// home folder, and a relative path is taken from the settings file's
    /// folder.
    Directory {
        /// The folder
        path: PathBuf,
    },
}
