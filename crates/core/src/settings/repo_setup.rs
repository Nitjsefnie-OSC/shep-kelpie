//! What reaches each agent of the repo's plugins and own files, `[app.dogs.kelpie.repo_setup]`
//!
//! Kelpie loads every plugin the repo turns on in `.claude/settings.json`,
//! and the repo's own `.claude` files, into every call (`crate::skills`). A project narrows that here, and adds
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
    /// Whether the repo's own `.claude/skills/`, `.claude/agents/`,
    /// `.claude/commands/` and the `hooks` of `.claude/settings.json` load,
    /// with no plugin. `false` leaves them out and keeps the plugins.
    #[serde(default = "yes")]
    pub own: bool,
    /// The calls that get the plugins and the repo's own files: `worker`, `reviewer`, `issue-writer`,
    /// `pm`, `advisor` and `researcher`. All of them when absent.
    #[serde(default = "every_role")]
    pub roles: Vec<CallRole>,
    /// The parts of a plugin, and of the repo's own files, that load: `skills`, `agents`, `commands`,
    /// `hooks` and `mcp`. All of them when absent.
    #[serde(default = "every_part")]
    pub parts: Vec<Part>,
    /// Plugins, as `name@marketplace`, left out though the repo turns them on
    #[serde(default)]
    pub skip: Vec<PluginId>,
    /// Plugins the repo does not turn on, each as `name@marketplace` with
    /// its marketplace's source as one string: `github:owner/name`,
    /// `git:<url>` or `directory:<folder inside the checkout>`, and
    /// `#<branch or tag>` after a `github` or `git` address
    #[serde(default)]
    pub extra: BTreeMap<PluginId, MarketplaceSource>,
}

fn yes() -> bool {
    true
}

fn every_part() -> Vec<Part> {
    Part::ALL.to_vec()
}

fn every_role() -> Vec<CallRole> {
    CallRole::ALL.to_vec()
}

impl Default for RepoSetup {
    fn default() -> Self {
        Self {
            plugins: true,
            own: true,
            roles: every_role(),
            parts: every_part(),
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

    /// Whether `part` of a plugin loads
    pub fn loads(&self, part: Part) -> bool {
        self.parts.contains(&part)
    }

    /// Whether the repo's plugin `id` is left out
    pub fn skips(&self, id: &str) -> bool {
        self.skip.iter().any(|skipped| skipped.as_str() == id)
    }
}

/// One part of a plugin, written as the folder or file that holds it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the bound goes here.
#[schemars(extend("pattern" = "^(skills|agents|commands|hooks|mcp)$"))]
pub enum Part {
    /// `skills/`
    Skills,
    /// `agents/`
    Agents,
    /// `commands/`
    Commands,
    /// `hooks/`
    Hooks,
    /// The plugin's MCP servers
    Mcp,
}

impl Part {
    const ALL: [Self; 5] = [
        Self::Skills,
        Self::Agents,
        Self::Commands,
        Self::Hooks,
        Self::Mcp,
    ];

    /// The name the setting writes it as, which is its folder where it has one
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

impl TryFrom<String> for Part {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|part| part.as_str() == value)
            .ok_or("must be skills, agents, commands, hooks or mcp")
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

/// Where a marketplace is, written as one string: `github:owner/name`,
/// `git:<url>` or `directory:<folder>`, and `#<branch or tag>` after a
/// `github` or `git` address to fetch that ref
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
#[schemars(extend("pattern" = "^(github|git|directory):.+$"))]
pub struct MarketplaceSource {
    kind: String,
    address: String,
    reference: Option<String>,
}

impl TryFrom<String> for MarketplaceSource {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        const WRITTEN: &str = "must be github:owner/name, git:<url> or directory:<folder>";
        let (kind, rest) = value.split_once(':').ok_or(WRITTEN)?;
        if !["github", "git", "directory"].contains(&kind) {
            return Err(WRITTEN);
        }
        let (address, reference) = match rest.rsplit_once('#') {
            Some((address, reference)) if kind != "directory" && !reference.is_empty() => {
                (address, Some(reference.to_owned()))
            }
            _ => (rest, None),
        };
        if address.is_empty() {
            return Err(WRITTEN);
        }
        Ok(Self {
            kind: kind.to_owned(),
            address: address.to_owned(),
            reference,
        })
    }
}

impl MarketplaceSource {
    /// The marketplace as an `extraKnownMarketplaces` entry holds it
    pub fn entry(&self) -> Value {
        let field = match self.kind.as_str() {
            "github" => "repo",
            "git" => "url",
            _ => "path",
        };
        let mut source = Map::new();
        source.insert("source".into(), json!(self.kind));
        source.insert(field.into(), json!(self.address));
        if let Some(reference) = &self.reference {
            source.insert("ref".into(), json!(reference));
        }
        json!({ "source": source })
    }
}
