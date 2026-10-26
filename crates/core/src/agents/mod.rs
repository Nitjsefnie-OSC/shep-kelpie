//! Agent definition files
//!
//! Each agent is `<name>.md` in the `agents` folder of kelpie's home: YAML
//! frontmatter naming what it is for and what runs it, then a Markdown body.
//! An implementer's body is added to kelpie's own instructions, and a
//! reviewer's, the issue writer's, the project manager's, the advisor's, the
//! researcher's, the representative's or the triager's is its prompt.
//! Kelpie embeds its own agents as baselines, and a file of the same name
//! replaces one. `shep kelpie add` writes a stub for each that is missing,
//! `extends: kelpie:<name>` and nothing else, and kelpie writes each baseline
//! whole and read-only to `.defaults/` to be read and diffed, which nothing
//! loads. A file with `extends: <name>` builds on the file of that name, and
//! one with `extends: kelpie:<name>` on a baseline, setting only the keys it
//! changes. A file that cannot be read or used stops the runner, naming the
//! file and what is wrong with it. A `.md` file whose name is no agent's,
//! such as a `README.md`, is skipped and named in the log. A review bot's
//! file is named for its bot, since the bot's window is its account's.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::board::AGENT_LABEL;
use crate::review_bot::BotReviewer;
use crate::settings::{
    AgentName, Effort, Endpoint, GatewayName, Implementer, Limit, LocalCommand, LocalRound,
    ModelHost, ModelServer, NonBlank, RoleAgents, RoleModel,
};

mod extends;
mod front;

/// The folder in kelpie's home that holds the agent files
pub const FOLDER: &str = "agents";

/// The folder of `agents` that holds each baseline whole, for reading and diffing
pub const BASELINES: &str = ".defaults";

/// The implementer a project lists when it names none
pub const DEFAULT_IMPLEMENTER: &str = "sonnet-high";

/// The reviewer that reads a pull request twice, Opus 5.5 at high effort
pub const DEFECT_HUNTER: &str = "defect-hunter";

/// The reviewer every project lists last when it names none
pub const DEFECT_HUNTER_LITE: &str = "defect-hunter-lite";

/// The reviewer every project lists first when it names none, where paddock is
/// set up
pub const IQ3_XXS_REVIEW: &str = "iq3-xxs-review";

/// The reviewer that runs the maintainer's qwen-review script
pub const QWEN: &str = "qwen";

/// The reviewer that reads a contribution for security, which a project runs
/// with `contributions.security_reviewer`
pub const SECURITY_REVIEWER: &str = "security-reviewer";

/// The agent `shep kelpie issue` runs
pub const ISSUE_WRITER: &str = "issue-writer";

/// The project manager kelpie ships, which a project runs with `agents.pm`
pub const PM: &str = "pm";

/// The advisor kelpie ships, which a project runs with `agents.advisor`
pub const ADVISOR: &str = "advisor";

/// The researcher kelpie ships, which a project runs with `agents.researcher`
pub const RESEARCHER: &str = "researcher";

/// The representative kelpie ships, which a project runs with
/// `agents.representative`
pub const REPRESENTATIVE: &str = "representative";

/// The triager kelpie ships, which a project runs with `triage.agent`
pub const TRIAGER: &str = "triager";

/// The maintainer's qwen-review script, which the `qwen` reviewer runs
pub const QWEN_REVIEW: &str = "~/.claude/scripts/qwen-review.sh";

// When `add` writes one of kelpie's own agents out.
#[derive(Debug, Clone, Copy)]
enum Written {
    Always,
    // Only where this file, under the home folder, is installed.
    Beside(&'static str),
}

// Kelpie's own agents, by name: what `add` writes a stub for, what a file
// reaches with `extends: kelpie:<name>`, and what a missing file falls back
// to. A project lists none of the review bots unless told to.
const DEFAULTS: [(&str, &str, Written); 18] = [
    (
        DEFAULT_IMPLEMENTER,
        include_str!("../../../../agents/sonnet-high.md"),
        Written::Always,
    ),
    (
        "opus-high",
        include_str!("../../../../agents/opus-high.md"),
        Written::Always,
    ),
    (
        DEFECT_HUNTER,
        include_str!("../../../../agents/defect-hunter.md"),
        Written::Always,
    ),
    (
        DEFECT_HUNTER_LITE,
        include_str!("../../../../agents/defect-hunter-lite.md"),
        Written::Always,
    ),
    (
        IQ3_XXS_REVIEW,
        include_str!("../../../../agents/iq3-xxs-review.md"),
        Written::Always,
    ),
    (
        "standards",
        include_str!("../../../../agents/standards.md"),
        Written::Always,
    ),
    (
        "spec",
        include_str!("../../../../agents/spec.md"),
        Written::Always,
    ),
    (
        SECURITY_REVIEWER,
        include_str!("../../../../agents/security-reviewer.md"),
        Written::Always,
    ),
    (
        QWEN,
        include_str!("../../../../agents/qwen.md"),
        Written::Beside(QWEN_REVIEW),
    ),
    (
        "coderabbit",
        include_str!("../../../../agents/coderabbit.md"),
        Written::Always,
    ),
    (
        "cubic",
        include_str!("../../../../agents/cubic.md"),
        Written::Always,
    ),
    (
        "codex",
        include_str!("../../../../agents/codex.md"),
        Written::Always,
    ),
    (
        ISSUE_WRITER,
        include_str!("../../../../agents/issue-writer.md"),
        Written::Always,
    ),
    (
        PM,
        include_str!("../../../../agents/pm.md"),
        Written::Always,
    ),
    (
        ADVISOR,
        include_str!("../../../../agents/advisor.md"),
        Written::Always,
    ),
    (
        RESEARCHER,
        include_str!("../../../../agents/researcher.md"),
        Written::Always,
    ),
    (
        REPRESENTATIVE,
        include_str!("../../../../agents/representative.md"),
        Written::Always,
    ),
    (
        TRIAGER,
        include_str!("../../../../agents/triager.md"),
        Written::Always,
    ),
];

/// What an agent is for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// It builds a work item, once a project lists it in `agents.implementers`
    Implementer,
    /// It reads a pull request once a pass, once a project lists it in
    /// `agents.reviewers`
    Reviewer,
    /// It turns a request into issues, as `shep kelpie issue` runs it
    IssueWriter,
    /// It manages the project's board, once a project names it in `agents.pm`
    Pm,
    /// It votes on each ruling from the codebase, once a project names it
    /// in `agents.advisor`
    Advisor,
    /// It votes on each ruling from everything outside the codebase, once a
    /// project names it in `agents.researcher`
    Researcher,
    /// It words the facts kelpie drafted into a post to a contributor, once a
    /// project names it in `agents.representative`
    Representative,
    /// It researches an issue other people opened and asks the author what
    /// the research leaves open, once a project names it in `triage.agent`
    Triager,
}

impl Role {
    /// The role's name, as an agent file writes it
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::IssueWriter => "issue-writer",
            Self::Pm => "pm",
            Self::Advisor => "advisor",
            Self::Researcher => "researcher",
            Self::Representative => "representative",
            Self::Triager => "triager",
        }
    }
}

/// What runs an agent's calls
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runs {
    /// A fresh or resumed session on a harness, at a model and effort
    Session {
        /// Its harness, model and effort
        model: RoleModel,
        /// What holds its calls back
        limit: Limit,
    },
    /// A command that keeps the README's contract: a reviewer only
    Command(LocalCommand),
    /// Kelpie's own reviewer over an OpenAI-compatible server: a reviewer only
    Endpoint(Endpoint),
    /// A review bot summoned on the pull request: a reviewer only
    Bot(BotReviewer),
}

impl Runs {
    /// The session's model and limit, for an agent that runs sessions
    pub fn session(&self) -> Option<(&RoleModel, &Limit)> {
        match self {
            Self::Session { model, limit } => Some((model, limit)),
            Self::Command(_) | Self::Endpoint(_) | Self::Bot(_) => None,
        }
    }

    /// The local round, for an agent that runs on its own
    pub fn local(&self) -> Option<LocalRound> {
        match self {
            Self::Session { .. } | Self::Bot(_) => None,
            Self::Command(command) => Some(LocalRound::Command(command.clone())),
            Self::Endpoint(endpoint) => Some(LocalRound::Endpoint(endpoint.clone())),
        }
    }

    /// The gateway its model is behind, and the model's name there, if any
    pub fn gateway(&self) -> Option<(&GatewayName, &str)> {
        match self {
            Self::Session { model, .. } => match model.harness.server() {
                Some(ModelServer {
                    host: ModelHost::Gateway(name),
                    ..
                }) => Some((name, model.model.as_str())),
                _ => None,
            },
            Self::Endpoint(Endpoint {
                host: ModelHost::Gateway(name),
                model,
                ..
            }) => Some((name, model.as_str())),
            Self::Endpoint(_) | Self::Command(_) | Self::Bot(_) => None,
        }
    }

    /// The bot, for an agent that is a review bot
    pub fn bot(&self) -> Option<BotReviewer> {
        match self {
            Self::Bot(bot) => Some(*bot),
            Self::Session { .. } | Self::Command(_) | Self::Endpoint(_) => None,
        }
    }
}

/// One agent, as a call on it takes it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// What it is for
    pub role: Role,
    /// What runs its calls
    pub runs: Runs,
    /// Its file's body: an implementer's addition to kelpie's own
    /// instructions, or a reviewer's or the issue writer's prompt. None
    /// when blank.
    pub prompt: Option<String>,
    /// Globs of the files a pull request must change for this reviewer to
    /// run. Every pull request when empty, and always empty for another role.
    pub paths: Vec<NonBlank>,
    /// Whether this reviewer reads twice, the second time shown what it found
    /// the first and asked only for what it missed. Never for another role.
    pub second_look: bool,
}

/// Every agent kelpie knows: its defaults, and the files that replace or add to them
#[derive(Clone, PartialEq, Eq)]
pub struct Agents {
    agents: BTreeMap<AgentName, Agent>,
    skipped: Vec<PathBuf>,
}

// Names each agent with its role, harness and model, and gives a prompt's
// length in place of its text, so a failed assert prints a few lines.
impl fmt::Debug for Agents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Summary<'a>(&'a Agent);

        impl fmt::Debug for Summary<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let Agent {
                    role, runs, prompt, ..
                } = self.0;
                write!(f, "{}", role.as_str())?;
                match runs.session() {
                    Some((model, _)) => write!(
                        f,
                        " on {} {}",
                        model.harness.harness().as_str(),
                        model.model.as_str()
                    )?,
                    None => write!(f, " {runs:?}")?,
                }
                match prompt {
                    Some(prompt) => write!(f, ", prompt {} bytes", prompt.len()),
                    None => write!(f, ", no prompt"),
                }
            }
        }

        f.debug_struct("Agents")
            .field(
                "agents",
                &self
                    .agents
                    .iter()
                    .map(|(name, agent)| (name, Summary(agent)))
                    .collect::<BTreeMap<_, _>>(),
            )
            .field("skipped", &self.skipped)
            .finish()
    }
}

impl Agents {
    /// Kelpie's own agents, with no file read
    pub fn embedded() -> Self {
        // A test pins that every default parses, so none is dropped here.
        let parsed = DEFAULTS.iter().filter_map(|(name, text, _)| {
            let name = AgentName::try_from((*name).to_owned()).ok()?;
            Some((name, front::parse(text).ok()?))
        });
        Self {
            agents: parsed.collect(),
            skipped: Vec::new(),
        }
    }

    /// Kelpie's own agents, then each `<name>.md` in `folder` over them
    ///
    /// A folder that is not there holds no files, and a `.md` file whose
    /// name is no agent's is skipped.
    ///
    /// # Errors
    ///
    /// [`AgentsError`] naming the folder or the first file, by name, that
    /// cannot be read or used.
    pub fn load(folder: &Path) -> Result<Self, AgentsError> {
        let mut agents = Self::embedded();
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(agents),
            Err(e) => return Err(failed(folder, e)),
        };
        let mut files = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| failed(folder, e))?.path();
            if path.extension().is_some_and(|e| e == "md") && path.is_file() {
                files.push(path);
            }
        }
        files.sort();
        // Every file is read first, since one may extend another that sorts after it.
        let mut texts = BTreeMap::new();
        for path in files {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let Ok(name) = AgentName::try_from(stem.to_owned()) else {
                agents.skipped.push(path);
                continue;
            };
            let text = fs::read_to_string(&path).map_err(|e| failed(&path, e))?;
            texts.insert(name, (path, text));
        }
        let parent = |name: &str| match name.strip_prefix(extends::SHIPPED) {
            Some(shipped) => DEFAULTS
                .iter()
                .find(|(default, ..)| *default == shipped)
                .map(|(_, text, _)| extends::ParentText::Default(text))
                .ok_or_else(|| {
                    let own: Vec<&str> = DEFAULTS.iter().map(|(name, ..)| *name).collect();
                    format!(
                        "`extends: {name}` names no shipped agent: kelpie's own are {}",
                        own.join(", ")
                    )
                }),
            None => AgentName::try_from(name.to_owned())
                .ok()
                .and_then(|name| texts.get(&name))
                .map(|(_, text)| extends::ParentText::File(text))
                .ok_or_else(|| {
                    format!(
                        "`extends: {name}` names no file: write `{name}.md`, or extend one of \
                         kelpie's own agents as `{}<name>`",
                        extends::SHIPPED
                    )
                }),
        };
        for (name, (path, text)) in &texts {
            let file_error = |message| AgentsError::File {
                path: path.clone(),
                message,
            };
            let merged = extends::merged(name.as_str(), text, &parent).map_err(file_error)?;
            let agent = front::parse_named(name.as_str(), &merged.text)
                .map_err(|message| file_error(merged.place(message)))?;
            agents.agents.insert(name.clone(), agent);
        }
        Ok(agents)
    }

    /// The agent `name`, if kelpie has one
    pub fn get(&self, name: &AgentName) -> Option<&Agent> {
        self.agents.get(name)
    }

    /// Every agent, by name
    pub fn iter(&self) -> impl Iterator<Item = (&AgentName, &Agent)> {
        self.agents.iter()
    }

    /// Every agent file whose role is `implementer`, by name: every agent an
    /// `agent:` label may name, listed in `agents.implementers` or not
    pub fn implementer_files(&self) -> Vec<AgentName> {
        self.agents
            .iter()
            .filter(|(_, agent)| agent.role == Role::Implementer)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// A line for each `.md` file skipped because its name is no agent's
    pub fn skipped(&self) -> impl Iterator<Item = String> + '_ {
        self.skipped.iter().map(|path| {
            format!(
                "agent file {} is skipped: an agent's name is lowercase letters, digits \
                 and `-`",
                path.display()
            )
        })
    }

    /// `name` defined by `text`, over any agent of that name, for a test
    ///
    /// # Panics
    ///
    /// When `name` or `text` is not a usable agent, naming why.
    #[cfg(any(test, feature = "test-support"))]
    #[track_caller]
    pub fn with(mut self, name: &str, text: &str) -> Self {
        let named: AgentName = name.to_owned().try_into().unwrap();
        let agent = front::parse_named(name, text).unwrap_or_else(|e| panic!("agent {name}: {e}"));
        self.agents.insert(named, agent);
        self
    }
}

/// Writes a stub for each of kelpie's own agents that `folder` lacks, never over
/// a file, and refreshes the baselines in `folder`'s `.defaults`
///
/// A stub is `extends: kelpie:<name>` and nothing else. The `qwen` reviewer
/// is written only where the maintainer's qwen-review script is installed
/// under `home`. Returns the names written, in order.
///
/// # Errors
///
/// [`AgentsError::Read`] naming the folder or file that could not be written.
pub fn write_defaults(folder: &Path, home: &Path) -> Result<Vec<&'static str>, AgentsError> {
    fs::create_dir_all(folder).map_err(|e| failed(folder, e))?;
    let mut written = Vec::new();
    for (name, _, when) in DEFAULTS {
        let wanted = match when {
            Written::Always => true,
            Written::Beside(file) => installed(file, home),
        };
        let stub = format!("---\nextends: {}{name}\n---\n", extends::SHIPPED);
        if wanted && write_new(&folder.join(format!("{name}.md")), &stub)? {
            written.push(name);
        }
    }
    write_baselines(folder)?;
    Ok(written)
}

/// Writes each of kelpie's own agents, whole and read-only, to `folder`'s
/// `.defaults`, over any file there
///
/// Each file leads with a comment saying kelpie writes it, so an edit is
/// lost, and where to override it instead. A file is replaced by a rename, so
/// a read never sees half of one.
///
/// # Errors
///
/// [`AgentsError::Read`] naming the folder or file that could not be written.
pub fn write_baselines(folder: &Path) -> Result<(), AgentsError> {
    let kept = folder.join(BASELINES);
    fs::create_dir_all(&kept).map_err(|e| failed(&kept, e))?;
    for (name, text, _) in DEFAULTS {
        let body = text.strip_prefix("---\n").unwrap_or(text);
        let whole = format!(
            "---\n# Kelpie writes this file and replaces it on every upgrade, so an edit here is \
             lost.\n# To change this agent, override it in agents/{name}.md instead.\n{body}"
        );
        let file = format!("{name}.md");
        replace(&kept, &file, &whole).map_err(|e| failed(&kept.join(&file), e))?;
    }
    sweep(&kept);
    Ok(())
}

/// How old a temporary file in `.defaults` is before it is a crash's leftover
/// and no live writer's, which holds one for milliseconds
const STALE: Duration = Duration::from_secs(60);

// Writes `text` read-only to a new file beside `name` in `dir`, then renames it
// over `name`. A failure leaves no temporary file.
fn replace(dir: &Path, name: &str, text: &str) -> io::Result<()> {
    let (beside, file) = create_temp(dir, name)?;
    let path = dir.join(name);
    let written = write_read_only(file, text)
        .and_then(|()| writable(&path))
        .and_then(|()| fs::rename(&beside, &path));
    if written.is_err() {
        // Best effort: the failure to report is the write's.
        let _ = writable(&beside).and_then(|()| fs::remove_file(&beside));
    }
    written
}

// A file no other writer holds, so two refreshes of one folder never share a
// temporary file and a leftover of a crash, whatever pid it had, is skipped.
fn create_temp(dir: &Path, name: &str) -> io::Result<(PathBuf, fs::File)> {
    let mut n = 0_u32;
    loop {
        let beside = dir.join(format!("{name}.{}-{n}.tmp", std::process::id()));
        match fs::File::create_new(&beside) {
            Ok(file) => return Ok((beside, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

fn write_read_only(mut file: fs::File, text: &str) -> io::Result<()> {
    file.write_all(text.as_bytes())?;
    let mut permissions = file.metadata()?.permissions();
    permissions.set_readonly(true);
    file.set_permissions(permissions)
}

// Makes `path` writable if it is a file, so it can be replaced or removed. A
// read-only file stops a rename over it on Windows, and a remove there.
#[cfg(windows)]
fn writable(path: &Path) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) => {
            let mut permissions = meta.permissions();
            // The lint warns that this makes a Unix file world-writable.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// Unix replaces and removes a read-only file by its folder's mode.
#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn writable(_: &Path) -> io::Result<()> {
    Ok(())
}

// Removes the temporary files in `kept` that no live writer can hold, which a
// crash left behind. Best effort: a refresh does not fail for them.
fn sweep(kept: &Path) {
    let Ok(entries) = fs::read_dir(kept) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > STALE);
        if old && path.extension().is_some_and(|ext| ext == "tmp") {
            let _ = writable(&path).and_then(|()| fs::remove_file(&path));
        }
    }
}

/// Whether `file`, written with a leading `~/`, is a file under `home`
pub fn installed(file: &str, home: &Path) -> bool {
    file.strip_prefix("~/")
        .is_some_and(|rest| home.join(rest).is_file())
}

// Writes `text` to `path` when nothing is there, and says whether it did.
fn write_new(path: &Path, text: &str) -> Result<bool, AgentsError> {
    match fs::File::create_new(path) {
        Ok(mut file) => {
            file.write_all(text.as_bytes())
                .map_err(|e| failed(path, e))?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(failed(path, e)),
    }
}

fn failed(path: &Path, e: io::Error) -> AgentsError {
    AgentsError::Read {
        path: path.to_owned(),
        kind: e.kind(),
    }
}

/// What `folder`'s `.md` files hold, by file name, so a change to any of
/// them can be seen without loading them
///
/// A folder or file that cannot be read shows as its error, so it reads as
/// a change once it can be.
pub fn snapshot(folder: &Path) -> Vec<(String, String)> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => return vec![(String::new(), e.kind().to_string())],
    };
    let mut files: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap_or_else(|e| e.kind().to_string());
            (path.display().to_string(), text)
        })
        .collect();
    files.sort();
    files
}

/// Writes `name`'s file in `folder` for a work item that opened before agent
/// files, on Claude Code with the `model` and `effort` its worker ran, never
/// over a file
///
/// Returns whether it wrote one: not when the file is there, or those would
/// not make a usable agent.
///
/// # Errors
///
/// [`AgentsError::Read`] naming the folder or file that could not be written.
pub fn write_kept(
    folder: &Path,
    name: &AgentName,
    model: &str,
    effort: Effort,
) -> Result<bool, AgentsError> {
    let text = format!(
        "---\n# Written by kelpie for a work item that opened before agent files,\n\
         # on the model and effort its worker ran.\n\
         role: implementer\nharness: claude-code\nmodel: \"{model}\"\neffort: {}\n---\n",
        effort.as_str()
    );
    let usable = !model.contains(['"', '\\', '\n']) && front::parse(&text).is_ok();
    if !usable {
        return Ok(false);
    }
    fs::create_dir_all(folder).map_err(|e| failed(folder, e))?;
    write_new(&folder.join(format!("{name}.md")), &text)
}

/// Why kelpie's agents cannot be used
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsError {
    /// The folder, or a file in it, could not be read or written
    Read {
        /// The folder or file
        path: PathBuf,
        /// What reading or writing it failed with
        kind: io::ErrorKind,
    },
    /// A file has no frontmatter, a key missing, unknown or malformed, or
    /// keys its harness cannot work with
    File {
        /// The file
        path: PathBuf,
        /// What is wrong, naming the key
        message: String,
    },
}

impl fmt::Display for AgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, kind } => write!(f, "agent files: {}: {kind}", path.display()),
            Self::File { path, message } => {
                write!(f, "agent file {}: {message}", path.display())
            }
        }
    }
}

impl core::error::Error for AgentsError {}

/// What the issue writer runs on, from its agent file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    /// Its agent file's name, `agents.issue_writer`
    pub name: AgentName,
    /// Its model and effort, on Claude Code
    pub model: RoleModel,
    /// Its file's body: its prompt
    pub prompt: String,
}

impl Writer {
    /// The agent `name` in `agents`, as `agents.issue_writer` names it
    ///
    /// # Errors
    ///
    /// A message when `agents` has no `name`, or its file is another role's.
    pub fn of(agents: &Agents, name: &AgentName) -> Result<Self, String> {
        let agent = agents.get(name).ok_or_else(|| {
            format!("there is no `{name}` agent: write `agents/{name}.md` in kelpie's home")
        })?;
        let (Role::IssueWriter, Runs::Session { model, .. }, Some(prompt)) =
            (agent.role, &agent.runs, &agent.prompt)
        else {
            return Err(format!(
                "agent file `{name}.md` is a {}'s: its role must be `issue-writer`",
                agent.role.as_str()
            ));
        };
        Ok(Self {
            name: name.clone(),
            model: model.clone(),
            prompt: prompt.clone(),
        })
    }
}

/// The listed implementers, the default first, as an `agent:` label names them
pub fn implementers(agents: &RoleAgents) -> String {
    let default = &agents.default_implementer;
    let line = |i: &Implementer| {
        let what = match (i.name == default.name, i.is_local()) {
            (true, true) => ", the default, a local model",
            (true, false) => ", the default",
            (false, true) => ", a local model",
            (false, false) => "",
        };
        format!(
            "- `{}`: {} at {} effort{what}",
            i.name,
            i.model.model.as_str(),
            i.model.effort.as_str()
        )
    };
    let rest = (agents.implementers.iter()).filter(|i| i.name != default.name);
    let lines: Vec<String> = std::iter::once(default).chain(rest).map(line).collect();
    format!(
        "The implementers this project lists, the default first. Each issue's \
         `{AGENT_LABEL}` label, where it has one, names one of them, and no other:\n\n{}",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests;
