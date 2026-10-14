//! An agent file's frontmatter, and the agent it defines
//!
//! The frontmatter is tagged by its `role`, so a key only a reviewer takes,
//! such as `paths`, is refused on an implementer by name, and the other
//! way round. It is read in two passes, the role and then that role's keys,
//! rather than as a serde-tagged enum, which buffers the keys and so loses
//! the line a refusal names.

use std::num::NonZeroU32;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::{Agent, Role, Runs};
use crate::forwarder::Upstream;
use crate::review_bot::Bot;

mod bot;
#[cfg(test)]
mod tests;

use crate::settings::{
    Account, AgentHarness, ContextSize, Effort, Endpoint, EndpointUrl, GatewayName, Harness, Limit,
    LocalCommand, ModelHost, ModelServer, NonBlank, RoleModel, UsageReader,
};
use bot::at_key;

// An agent file's frontmatter, by its role
#[derive(Debug)]
enum Front {
    Implementer(Implementer),
    Reviewer(Reviewer),
    IssueWriter(OnClaude),
    Pm(OnClaude),
    Advisor(Expert),
    Researcher(Expert),
}

// The first pass: the role alone, whatever else is there
#[derive(Debug, Deserialize)]
struct Tag {
    role: Role,
}

// An implementer's keys: a session on a harness
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Implementer {
    #[serde(rename = "role")]
    _role: Role,
    harness: Harness,
    model: NonBlank,
    effort: Effort,
    #[serde(default)]
    usage: Option<UsageReader>,
    #[serde(default)]
    lease: Option<IgnoredAny>,
    #[serde(default)]
    url: Option<EndpointUrl>,
    #[serde(default)]
    gateway: Option<GatewayName>,
    #[serde(default)]
    context: Option<ContextSize>,
    #[serde(default)]
    tools: Option<NonBlank>,
}

// The issue writer's and the project manager's keys: a Claude Code
// session, since each is held by a Claude Code hook
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnClaude {
    #[serde(rename = "role")]
    _role: Role,
    harness: Harness,
    model: NonBlank,
    effort: Effort,
}

// The advisor's and the researcher's keys: a session on any harness. Neither
// takes `tools`: each is held to its reads by the kind of call kelpie makes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expert {
    #[serde(rename = "role")]
    _role: Role,
    harness: Harness,
    model: NonBlank,
    effort: Effort,
    #[serde(default)]
    usage: Option<UsageReader>,
    #[serde(default)]
    lease: Option<IgnoredAny>,
    #[serde(default)]
    url: Option<EndpointUrl>,
    #[serde(default)]
    gateway: Option<GatewayName>,
    #[serde(default)]
    context: Option<ContextSize>,
}

// What runs a reviewer: a session on a harness, a command, an endpoint or a
// review bot
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ReviewerHarness {
    ClaudeCode,
    Pi,
    Codex,
    Goose,
    Command,
    Endpoint,
    Bot,
    #[cfg(test)]
    StandIn,
}

// A reviewer's keys, which its harness narrows
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reviewer {
    #[serde(rename = "role")]
    _role: Role,
    harness: ReviewerHarness,
    #[serde(default)]
    model: Option<NonBlank>,
    #[serde(default)]
    effort: Option<Effort>,
    #[serde(default)]
    usage: Option<UsageReader>,
    #[serde(default)]
    lease: Option<IgnoredAny>,
    #[serde(default)]
    url: Option<EndpointUrl>,
    #[serde(default)]
    gateway: Option<GatewayName>,
    #[serde(default)]
    context: Option<ContextSize>,
    #[serde(default)]
    command: Option<PathBuf>,
    #[serde(default)]
    paths: Vec<NonBlank>,
    #[serde(default)]
    second_look: bool,
    #[serde(default)]
    bot: Option<Bot>,
    #[serde(default)]
    reviews: Option<NonZeroU32>,
    #[serde(default)]
    hours: Option<NonZeroU32>,
    #[serde(default)]
    reviews_on_ready: Option<bool>,
    #[serde(default)]
    rounds: Option<NonZeroU32>,
    #[serde(default)]
    tools: Option<NonBlank>,
}

// A session's keys, whichever role runs it
struct SessionKeys<'a> {
    harness: Harness,
    model: &'a NonBlank,
    effort: Effort,
    usage: Option<UsageReader>,
    host: Option<ModelHost>,
    context: Option<ContextSize>,
    tools: Option<&'a NonBlank>,
}

/// The agent an agent file's `text` defines, or what is wrong with it
pub(super) fn parse(text: &str) -> Result<Agent, String> {
    let Some((front, body)) = split(text) else {
        return Err("must start with a `---` line, then the YAML frontmatter, \
                    then a `---` line of its own"
            .into());
    };
    // A blank first line stands for the opening `---`, so a message's line is the file's.
    let yaml = format!("\n{front}");
    let refused = |e: serde_saphyr::Error| yaml_error(&yaml, &e);
    let front = match serde_saphyr::from_str::<Tag>(&yaml).map_err(refused)?.role {
        Role::Implementer => Front::Implementer(serde_saphyr::from_str(&yaml).map_err(refused)?),
        Role::Reviewer => Front::Reviewer(serde_saphyr::from_str(&yaml).map_err(refused)?),
        Role::IssueWriter => Front::IssueWriter(serde_saphyr::from_str(&yaml).map_err(refused)?),
        Role::Pm => Front::Pm(serde_saphyr::from_str(&yaml).map_err(refused)?),
        Role::Advisor => Front::Advisor(serde_saphyr::from_str(&yaml).map_err(refused)?),
        Role::Researcher => Front::Researcher(serde_saphyr::from_str(&yaml).map_err(refused)?),
    };
    let leased = match &front {
        Front::Implementer(keys) => keys.lease.is_some(),
        Front::Reviewer(keys) => keys.lease.is_some(),
        Front::Advisor(keys) | Front::Researcher(keys) => keys.lease.is_some(),
        Front::IssueWriter(_) | Front::Pm(_) => false,
    };
    if leased {
        return Err(at_key(&yaml, "lease", LEASE_GONE));
    }
    let body = body.trim();
    let prompt = (!body.is_empty()).then(|| body.to_owned());
    let agent = match front {
        Front::Implementer(keys) => Ok(Agent {
            role: Role::Implementer,
            runs: session(&SessionKeys {
                harness: keys.harness,
                model: &keys.model,
                effort: keys.effort,
                usage: keys.usage,
                host: host(keys.url.as_ref(), keys.gateway.as_ref())?,
                context: keys.context,
                tools: keys.tools.as_ref(),
            })?,
            prompt,
            paths: Vec::new(),
            second_look: false,
        }),
        Front::Reviewer(keys) => {
            let runs = keys.runs(prompt.as_deref(), &yaml)?;
            Ok(Agent {
                role: Role::Reviewer,
                runs,
                prompt,
                paths: keys.paths,
                second_look: keys.second_look,
            })
        }
        Front::IssueWriter(keys) => keys.agent(
            Role::IssueWriter,
            prompt,
            &yaml,
            (
                "the issue writer",
                "`--interactive` starts `claude`, and its guard is a Claude Code hook",
            ),
        ),
        Front::Pm(keys) => keys.agent(
            Role::Pm,
            prompt,
            &yaml,
            (
                "the project manager",
                "a Claude Code hook is what holds its writes to its notes",
            ),
        ),
        Front::Advisor(keys) => keys.agent(Role::Advisor, prompt, "the advisor"),
        Front::Researcher(keys) => keys.agent(Role::Researcher, prompt, "the researcher"),
    }?;
    goose_opens_at(&agent, &yaml)?;
    Ok(agent)
}

// goose runs the researcher alone until the slices that open the other roles
fn goose_opens_at(agent: &Agent, yaml: &str) -> Result<(), String> {
    let on_goose = matches!(
        agent.runs.session(),
        Some((
            RoleModel {
                harness: AgentHarness::Goose(_),
                ..
            },
            _
        ))
    );
    let (who, slice) = match agent.role {
        Role::Implementer => ("an implementer", "#788"),
        Role::Reviewer => ("a reviewer", "#789"),
        Role::Advisor => ("an advisor", "#789"),
        // The issue writer and the project manager already refuse any harness but Claude Code.
        Role::IssueWriter | Role::Pm | Role::Researcher => return Ok(()),
    };
    match on_goose {
        true => Err(at_key(
            yaml,
            "harness",
            &format!(
                "{who} cannot run on goose yet: {slice} opens it, and only a researcher runs on it until then"
            ),
        )),
        false => Ok(()),
    }
}

impl Expert {
    /// The agent of `role` these keys and `prompt` make, or why not: `who`
    /// names it
    fn agent(&self, role: Role, prompt: Option<String>, who: &str) -> Result<Agent, String> {
        if prompt.is_none() {
            return Err(format!(
                "{who}'s prompt is the file's body: write it below the closing `---`"
            ));
        }
        Ok(Agent {
            role,
            runs: session(&SessionKeys {
                harness: self.harness,
                model: &self.model,
                effort: self.effort,
                usage: self.usage,
                host: host(self.url.as_ref(), self.gateway.as_ref())?,
                context: self.context,
                tools: None,
            })?,
            prompt,
            paths: Vec::new(),
            second_look: false,
        })
    }
}

impl OnClaude {
    /// The agent of `role` these keys and `prompt` make, or why not: `who`
    /// names it and why it runs on Claude Code alone
    fn agent(
        &self,
        role: Role,
        prompt: Option<String>,
        yaml: &str,
        (who, why): (&str, &str),
    ) -> Result<Agent, String> {
        // parity: harness.issue-writer, harness.pm
        if self.harness != Harness::ClaudeCode {
            let name = self.harness.as_str();
            return Err(at_key(
                yaml,
                "harness",
                &format!("{who} runs on claude-code, not {name}: {why}"),
            ));
        }
        if prompt.is_none() {
            return Err(format!(
                "{who}'s prompt is the file's body: write it below the closing `---`"
            ));
        }
        Ok(Agent {
            role,
            runs: session(&SessionKeys {
                harness: self.harness,
                model: &self.model,
                effort: self.effort,
                usage: None,
                host: None,
                context: None,
                tools: None,
            })?,
            prompt,
            paths: Vec::new(),
            second_look: false,
        })
    }
}

impl Reviewer {
    /// What runs it, or why its keys cannot: `prompt` is the file's body, if
    /// it has one
    fn runs(&self, prompt: Option<&str>, yaml: &str) -> Result<Runs, String> {
        let harness = match self.harness {
            ReviewerHarness::ClaudeCode => Harness::ClaudeCode,
            ReviewerHarness::Pi => Harness::Pi,
            ReviewerHarness::Codex => Harness::Codex,
            ReviewerHarness::Goose => Harness::Goose,
            #[cfg(test)]
            ReviewerHarness::StandIn => Harness::StandIn,
            ReviewerHarness::Command => {
                self.no_bot_keys("command", yaml)?;
                return self.command(prompt.is_some()).map(Runs::Command);
            }
            ReviewerHarness::Endpoint => {
                self.no_bot_keys("endpoint", yaml)?;
                return self.endpoint(prompt).map(Runs::Endpoint);
            }
            ReviewerHarness::Bot => {
                let key = if self.bot.is_some() { "bot" } else { "harness" };
                return self
                    .bot(prompt.is_some())
                    .map(Runs::Bot)
                    .map_err(|m| at_key(yaml, key, &m));
            }
        };
        let name = harness.as_str();
        self.no_bot_keys(name, yaml)?;
        if self.command.is_some() {
            return Err(format!(
                "runs a session on {name}, which takes no `command`"
            ));
        }
        let (Some(model), Some(effort)) = (&self.model, self.effort) else {
            return Err(format!(
                "runs a session on {name}, which needs `model` and `effort`"
            ));
        };
        if prompt.is_none() {
            return Err(format!(
                "runs a session on {name}, whose prompt is the file's body: write it \
                 below the closing `---`"
            ));
        }
        session(&SessionKeys {
            harness,
            model,
            effort,
            usage: self.usage,
            host: host(self.url.as_ref(), self.gateway.as_ref())?,
            context: self.context,
            tools: self.tools.as_ref(),
        })
    }

    // A command keeps the README's contract, which gives it no model, prompt
    // or findings to look again at.
    fn command(&self, prompted: bool) -> Result<LocalCommand, String> {
        self.on_its_own("command", prompted)?;
        let session_keys = self.model.is_some()
            || self.effort.is_some()
            || self.url.is_some()
            || self.gateway.is_some()
            || self.context.is_some()
            || self.tools.is_some();
        if session_keys {
            return Err(
                "runs a command, which takes no `model`, `effort`, `url`, `gateway`, \
                        `context` or `tools`"
                    .into(),
            );
        }
        let Some(command) = &self.command else {
            return Err("runs a command, which needs its path as `command`".into());
        };
        if !(command.is_absolute() || command.starts_with("~")) {
            return Err("`command` must start with `/` or `~/`".into());
        }
        Ok(LocalCommand {
            command: command.clone(),
        })
    }

    // An endpoint takes a body for what to report. Kelpie sends the diff in
    // chunks, so the body cannot place the whole diff or its base.
    fn endpoint(&self, prompt: Option<&str>) -> Result<Endpoint, String> {
        self.on_its_own("endpoint", false)?;
        for placeholder in ["{{BASE}}", "{{DIFF}}"] {
            if prompt.is_some_and(|body| body.contains(placeholder)) {
                return Err(format!(
                    "runs on endpoint, which sends the diff in chunks of its own, so its \
                     body cannot name {placeholder}: leave it out"
                ));
            }
        }
        let other = self.effort.is_some() || self.command.is_some() || self.tools.is_some();
        if other {
            return Err(
                "runs on an endpoint, which takes no `effort`, `command` or `tools`".into(),
            );
        }
        let host = host(self.url.as_ref(), self.gateway.as_ref())?;
        let (Some(host), Some(model), Some(context)) = (host, &self.model, self.context) else {
            return Err("runs on an endpoint, which needs the server as `url` or a \
                        `gateway`, its `model` and the model's context size as `context`"
                .into());
        };
        Ok(Endpoint {
            host,
            model: model.clone(),
            context,
            prompt: prompt.map(str::to_owned),
        })
    }

    // What a reviewer that runs on its own, not in a session, cannot take.
    fn on_its_own(&self, harness: &str, prompted: bool) -> Result<(), String> {
        if self.usage.is_some() {
            return Err(format!(
                "runs on {harness}, which spends no account, so it takes no `usage`"
            ));
        }
        if self.second_look {
            return Err(format!(
                "runs on {harness}, which cannot be shown its own findings, so it \
                 takes no `second_look`"
            ));
        }
        if prompted {
            return Err(format!(
                "runs on {harness}, which writes its own prompt: leave the body empty, \
                 and keep notes as `#` lines in the frontmatter"
            ));
        }
        Ok(())
    }
}

/// A session's model and limit, or why its keys cannot reach that model
fn session(keys: &SessionKeys<'_>) -> Result<Runs, String> {
    Ok(Runs::Session {
        model: role_model(keys)?,
        limit: limit(keys)?,
    })
}

// A server's URL or the gateway in front of it, whichever the keys name.
fn host(
    url: Option<&EndpointUrl>,
    gateway: Option<&GatewayName>,
) -> Result<Option<ModelHost>, String> {
    match (url, gateway) {
        (Some(_), Some(_)) => Err("names both `url` and `gateway`: keep the one its \
                                   requests go to"
            .into()),
        (Some(url), None) => Ok(Some(ModelHost::Url(url.clone()))),
        (None, Some(gateway)) => Ok(Some(ModelHost::Gateway(gateway.clone()))),
        (None, None) => Ok(None),
    }
}

/// Why a `lease` is refused: kelpie holds no lock of its own on a model
const LEASE_GONE: &str = "kelpie holds no lock of its own on a GPU now, so `lease` is gone: \
     name a `gateway` such as paddock in place of `url`, which queues the model's calls, \
     and leave `lease` out";

fn role_model(keys: &SessionKeys<'_>) -> Result<RoleModel, String> {
    // parity: harness.tool-allow-list
    if keys.tools.is_some() && keys.harness != Harness::ClaudeCode {
        let name = keys.harness.as_str();
        return Err(format!(
            "runs on {name}, which takes no `tools`: only claude-code has an allow-list to name"
        ));
    }
    let harness = match (keys.harness, &keys.host, keys.context) {
        (Harness::ClaudeCode, None, None) => AgentHarness::ClaudeCode,
        (Harness::ClaudeCode, ..) => {
            return Err("runs on claude-code, which takes no `url`, `gateway` or `context`".into());
        }
        (Harness::Pi, Some(host), Some(context)) => {
            AgentHarness::Pi(model_server("pi", host, context)?)
        }
        (Harness::Goose, Some(host), Some(context)) => {
            AgentHarness::Goose(model_server("goose", host, context)?)
        }
        (Harness::Pi | Harness::Goose, ..) => {
            return Err(format!(
                "runs on {}, which needs the model's server as `url` or a \
                 `gateway`, and its context size as `context`",
                keys.harness.as_str()
            ));
        }
        (Harness::Codex, None, None) => AgentHarness::Codex,
        (Harness::Codex, ..) => {
            return Err("runs on codex, which takes no `url`, `gateway` or `context`".into());
        }
        #[cfg(test)]
        (Harness::StandIn, ..) => AgentHarness::StandIn,
    };
    Ok(RoleModel {
        model: keys.model.clone(),
        effort: keys.effort,
        harness,
        tools: keys.tools.cloned(),
    })
}

// The server a pi or goose agent named `harness` runs its model on
fn model_server(
    harness: &str,
    host: &ModelHost,
    context: ContextSize,
) -> Result<ModelServer, String> {
    if let ModelHost::Url(url) = host
        && let Err(e) = Upstream::new(url)
    {
        return Err(format!(
            "runs on {harness}, with a `url` kelpie cannot forward to: {e}"
        ));
    }
    Ok(ModelServer {
        host: host.clone(),
        context,
    })
}

/// What holds a session's calls back, or why its keys do not say
///
/// The reader must be the harness's own: an agent on Claude Code spends
/// the Claude account whatever its `usage` says.
fn limit(keys: &SessionKeys<'_>) -> Result<Limit, String> {
    let own = match keys.harness {
        Harness::ClaudeCode => UsageReader::Claude,
        Harness::Pi | Harness::Goose => UsageReader::None,
        Harness::Codex => UsageReader::Codex,
        #[cfg(test)]
        Harness::StandIn => keys.usage.unwrap_or(UsageReader::Claude),
    };
    let usage = keys.usage.unwrap_or(own);
    if usage != own {
        return Err(format!(
            "runs on {}, whose usage is read with `{}`, so it cannot set \
             `usage: {}`: leave `usage` out",
            keys.harness.as_str(),
            own.as_str(),
            usage.as_str()
        ));
    }
    let gateway = matches!(keys.host, Some(ModelHost::Gateway(_)));
    match usage {
        UsageReader::None if gateway => Ok(Limit::Gateway),
        UsageReader::None => Ok(Limit::Unqueued),
        UsageReader::Claude => Ok(Limit::Account(Account::Claude)),
        UsageReader::Codex => Ok(Limit::Account(Account::Codex)),
    }
}

/// The agent `name`'s file `text` defines, or what is wrong with it
///
/// A review bot's file is named for its bot: the dog books each bot's
/// window from that file, so a second file could not hold a window of its own.
pub(super) fn parse_named(name: &str, text: &str) -> Result<Agent, String> {
    let agent = parse(text)?;
    match agent.runs.bot() {
        Some(bot) if name != bot.bot.as_str() => {
            let yaml = split(text)
                .map(|(front, _)| format!("\n{front}"))
                .unwrap_or_default();
            Err(at_key(
                &yaml,
                "bot",
                &format!(
                    "runs {}, so it must be named `{}.md`: each bot has one file, which \
                     holds its account's window",
                    bot.bot, bot.bot
                ),
            ))
        }
        _ => Ok(agent),
    }
}

// The parser's message, led by the key whose value it refuses. A missing or
// unknown key is already named, and placed at the start of a line.
fn yaml_error(yaml: &str, error: &serde_saphyr::Error) -> String {
    let message = error.without_snippet().to_string();
    let key = error
        .location()
        .filter(|at| at.column() > 1)
        .and_then(|at| {
            let line = yaml
                .lines()
                .nth(usize::try_from(at.line()).ok()?.checked_sub(1)?)?;
            let (key, _) = line.split_once(':')?;
            Some(key.trim()).filter(|k| !k.is_empty() && !k.starts_with('#'))
        });
    match key {
        Some(key) => format!("`{key}`: {message}"),
        None => message,
    }
}

// The frontmatter between the opening and closing `---` lines, and the body after.
pub(super) fn split(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}
