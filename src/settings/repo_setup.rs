//! What reaches each agent of the repo's plugins, `[app.dogs.kelpie.repo_setup]`
//!
//! Kelpie loads every plugin the repo turns on in `.claude/settings.json`
//! into every call (`crate::skills`). A project narrows that here, and adds
//! a plugin the repo does not name. Every key is optional, and a value is a
//! string, a list of strings or a table of strings, which lookout edits.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::ports::Role;

/// Which of the repo's plugins reach which calls, and the plugins the repo
/// does not turn on
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Repo setup")]
pub struct RepoSetup {
    /// Whether the plugins the repo turns on in `.claude/settings.json` load.
    /// `false` loads none of them, and `extra` still loads.
    #[serde(default = "yes")]
    pub plugins: bool,
    /// The calls that get the plugins: `worker`, `reviewer`, `issue-writer`,
    /// `pm`, `advisor` and `researcher`. All of them when absent.
    #[serde(default = "every_role")]
    pub roles: Vec<CallRole>,
    /// Plugins, as `name@marketplace`, left out though the repo turns them on
    #[serde(default)]
    pub skip: Vec<PluginId>,
    /// Plugins the repo does not turn on, each as `name@marketplace` with
    /// its marketplace's source: `source` is `github` (with `repo`, as
    /// `owner/name`), `git` (with `url`) or `directory` (with `path`, a
    /// folder inside the checkout), and `ref` is an optional branch or tag
    #[serde(default)]
    pub extra: BTreeMap<PluginId, MarketplaceSource>,
}

fn yes() -> bool {
    true
}

fn every_role() -> Vec<CallRole> {
    CallRole::ALL.to_vec()
}

impl Default for RepoSetup {
    fn default() -> Self {
        Self {
            plugins: true,
            roles: every_role(),
            skip: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl RepoSetup {
    /// Whether a call of `role` gets the plugins
    pub fn reaches(&self, role: Role) -> bool {
        self.roles.iter().any(|r| r.0 == role)
    }

    /// Whether the repo's plugin `id` is left out
    pub fn skips(&self, id: &str) -> bool {
        self.skip.iter().any(|skipped| skipped.as_str() == id)
    }
}

/// A call's role, written as the labels of a lamb carry it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the bound goes here.
#[schemars(extend("pattern" = "^(worker|reviewer|issue-writer|pm|advisor|researcher)$"))]
pub struct CallRole(#[schemars(with = "String")] Role);

impl CallRole {
    const ALL: [Self; 6] = [
        Self(Role::Worker),
        Self(Role::Reviewer),
        Self(Role::IssueWriter),
        Self(Role::Pm),
        Self(Role::Advisor),
        Self(Role::Researcher),
    ];
}

impl TryFrom<String> for CallRole {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|role| role.0.as_str() == value)
            .ok_or("must be worker, reviewer, issue-writer, pm, advisor or researcher")
    }
}

/// A plugin as `name@marketplace`, each part letters, digits, `-`, `_` and
/// `.`, not starting with `.`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
#[schemars(extend("pattern" = "^[A-Za-z0-9_-][A-Za-z0-9_.-]*@[A-Za-z0-9_-][A-Za-z0-9_.-]*$"))]
pub struct PluginId(String);

impl PluginId {
    /// The plugin as written
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The marketplace it comes from, after the `@`
    pub fn marketplace(&self) -> &str {
        self.0.split_once('@').map_or("", |(_, market)| market)
    }
}

impl TryFrom<String> for PluginId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let plain = |part: &str| super::SkillName::try_from(part.to_owned()).is_ok();
        match value.split_once('@') {
            Some((name, market)) if plain(name) && plain(market) => Ok(Self(value)),
            _ => Err("must be name@marketplace: letters, digits, - _ . in each"),
        }
    }
}

/// Where a marketplace is, in the shape of the `source` of an
/// `extraKnownMarketplaces` entry
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceSource {
    /// `github`, `git` or `directory`
    pub source: String,
    /// The GitHub repo as `owner/name`, for `github`
    #[serde(default)]
    pub repo: Option<String>,
    /// The git address, for `git`
    #[serde(default)]
    pub url: Option<String>,
    /// A folder inside the checkout, for `directory`
    #[serde(default)]
    pub path: Option<String>,
    /// A branch or tag to fetch, for `github` and `git`
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
}

impl MarketplaceSource {
    /// The marketplace as an `extraKnownMarketplaces` entry holds it
    pub fn entry(&self) -> Value {
        let mut source = Map::new();
        source.insert("source".into(), json!(self.source));
        let keyed = [
            ("repo", &self.repo),
            ("url", &self.url),
            ("path", &self.path),
            ("ref", &self.reference),
        ];
        for (key, value) in keyed {
            if let Some(value) = value {
                source.insert(key.into(), json!(value));
            }
        }
        json!({ "source": source })
    }
}
