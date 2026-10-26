//! goose, headless as `goose run`, one process per call, on an OpenAI-compatible model
//!
//! Each call runs inside kelpie's sandbox in a goose root of its own, so the
//! maintainer's goose setup never loads. A researcher gets only `skills` and
//! its MCP web ([`web`]). A worker gets `developer`, and every tool call goes
//! through the `PreToolUse` plugin ([`hooks`]); a fenced reviewer, advisor and
//! retro get its read-only guard, reading through a `shell`. A call loads MCP
//! servers as flags of a new session ([`mcp`]) and its plugins' skills ([`skills`]).
//! goose dials a forwarder outside the sandbox, which passes on only the chat
//! completions call. A gateway model gets its key and holds its lease per call.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use self::files::Files;
use self::output::Spent;
use super::cli::{Cli, CliAdapter, FollowUp, Launch, Turn};
use super::harness::{Base, assignments, bind_in_scratch, call_policy, program_folder};
use super::process::record::CallFiles;
use crate::dep_sources::ProcessEnv;
use crate::fence;
use crate::forwarder::Upstream;
use crate::guard::READ_ONLY_COMMANDS;
use crate::ports::{
    Adopted, AgentCall, AgentError, AgentReply, COMPACT, CallActivity, Policy, Role, Session,
    Tools, Transcript,
};
use crate::settings::{Harness, ModelServer};

// What the module's tests take from it by glob.
#[cfg(test)]
use {
    super::forwarder::Forwarder,
    crate::ports::{Agents, Ending, Forward},
    crate::settings::{AgentHarness, Gateways, ModelHost},
};

mod activity;
mod attach;
mod compact;
mod files;
pub(crate) mod hooks;
pub(crate) mod mcp;
mod output;
mod skills;
pub(crate) mod web;

#[cfg(test)]
mod activity_tests;

#[cfg(test)]
mod adopt_tests;

#[cfg(test)]
mod attach_tests;

#[cfg(test)]
mod compact_tests;

#[cfg(test)]
mod gateway_tests;

#[cfg(test)]
mod hooks_tests;

#[cfg(test)]
mod mcp_tests;

#[cfg(test)]
mod reading_tests;

#[cfg(test)]
mod escapes;

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod socket_tests;

#[cfg(test)]
mod skills_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unreachable_tests;

#[cfg(test)]
mod web_tests;

/// The oldest goose kelpie is built for: `on_failure: block` hooks need 1.48 or
/// later, and 1.53.0 is the source the adapter is written from. The target is 1.54.0.
pub const GOOSE_MINIMUM: &str = "1.53.0";

/// This adapter's harness, as its errors name it
const GOOSE: Harness = Harness::Goose;

/// The key goose is given for its server. The forwarder sends a gateway's own.
const PLACEHOLDER_KEY: &str = "kelpie";

/// The files goose loads as the repo's instructions for a worker
const CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The project folders goose loads plugins and their settings from, whatever
/// `--no-profile` says
const PROJECT_FILES: [&str; 2] = [".agents/plugins", ".config/goose"];

/// goose's own part of a call
#[derive(Debug, Clone)]
pub struct Goose {
    program: OsString,
    // What a call with no fence may not read, besides its own folder
    unfenced_no_read: Vec<String>,
}

/// Headless goose, one `goose run` process per call, each inside the sandbox
///
/// It shares Claude Code's calls in flight, so stopping either stops both.
pub type GooseCli = CliAdapter<Goose>;

impl GooseCli {
    /// goose on `base`: its sandbox, lamb labels and calls in flight
    pub fn on(base: &Base) -> Self {
        let goose = Goose {
            program: "goose".into(),
            unfenced_no_read: base.unfenced_no_read.clone(),
        };
        Self::new(goose, base)
    }

    /// Runs `program` in place of `goose`, as a stand-in script does
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_program(mut self, program: PathBuf) -> Self {
        self.cli.program = program.into();
        self
    }

    /// The call's headless command inside its sandbox and the forwarder it dials
    #[cfg(test)]
    pub(crate) fn sandboxed_command(
        &self,
        call: &AgentCall,
        lease: Option<&str>,
    ) -> Result<(Command, Forwarder), AgentError> {
        let launched = self.launch(call, &Spent::default(), Turn::Headless, lease)?;
        let forwarder = launched.forwarder.expect("goose runs on a model server");
        Ok((launched.command, forwarder))
    }
}

impl Goose {
    // `goose` with `args`, its variables set by `env` inside the sandbox, after the sandbox
    // has set its own. `mode` is the `GOOSE_MODE` the call is given in place of its headless one.
    fn program_command(
        &self,
        call: &AgentCall,
        launch: &Launch<'_>,
        args: Vec<OsString>,
        mode: Option<&str>,
    ) -> Result<Command, AgentError> {
        let served = launch.served.ok_or_else(unserved)?;
        let files = Files::of(&call.settings);
        let mut command = crate::spawn::command("env");
        command
            .args(env_args(call, server(call)?, &files, served.upstream, mode))
            .arg(&self.program)
            .args(args)
            .current_dir(&call.cwd);
        Ok(command)
    }
}

impl Cli for Goose {
    // The totals the session had spent when the call began
    type Begun = Spent;
    const HARNESS: Harness = Harness::Goose;

    fn mcp_refusal(name: &str, entry: &serde_json::Value) -> Option<String> {
        mcp::refusal(name, entry)
    }

    fn mcp_skipped_env(name: &str, entry: &serde_json::Value) -> Option<String> {
        mcp::skipped_env(name, entry)
    }

    fn refuse(&self, call: &AgentCall) -> Result<(), AgentError> {
        refuse_unsupported(call)
    }

    fn write_config(&self, call: &AgentCall, _: Option<&Upstream>) -> Result<(), AgentError> {
        system_text(call)?;
        let files = Files::of(&call.settings);
        hooks::only_ours(&files)?;
        hooks::no_settings_in_root(&files)?;
        files.prepare()?;
        skills::link(call, &files)?;
        // A new session records the servers it is created with, since a resumed one holds those
        // and not what the call loads by then.
        if let Session::New(id) = &call.session {
            files.keep_servers(id, &mcp::keys(call))?;
        }
        // A call whose session holds a server runs in `auto` too, so it is held by the hook as a
        // worker is.
        match &call.reach.fence {
            Some(fence) if has_shell(call) || mcp::loads(call, &files) => {
                let judging = judging(call, fence);
                hooks::write(&files, fence, judging, &mcp::held(call, &files)?)
            }
            _ => Ok(()),
        }
    }

    // A session kelpie never started never waits for a lease, and a new one is
    // recorded as started with nothing spent. The maintainer's terminal resumes
    // one, and `goose session` has no flag for a first message.
    fn begin(&self, call: &AgentCall, turn: Turn) -> Result<Spent, AgentError> {
        let files = Files::of(&call.settings);
        let id = call.session.id();
        match (turn, &call.session) {
            (Turn::Headless, Session::New(_)) => {
                files.keep(id, Spent::default())?;
                Ok(Spent::default())
            }
            (Turn::Terminal, _) if !call.prompt.is_empty() => Err(AgentError::Setup(
                "goose cannot start a terminal session with a first message: `goose session` \
                 takes none"
                    .into(),
            )),
            _ => files
                .spent(id)?
                .ok_or_else(|| AgentError::NoSession(GOOSE, id.clone())),
        }
    }

    fn scratch(&self, call: &AgentCall) -> PathBuf {
        Files::of(&call.settings).scratch
    }

    fn sandbox_settings(&self, call: &AgentCall) -> PathBuf {
        Files::of(&call.settings).sandbox_settings
    }

    fn command(
        &self,
        call: &AgentCall,
        _: &Spent,
        launch: &Launch<'_>,
    ) -> Result<Command, AgentError> {
        match launch.turn {
            Turn::Headless => self.program_command(call, launch, argv(call)?, None),
            Turn::Terminal => {
                self.program_command(call, launch, attach::argv(call), Some(attach::MODE))
            }
        }
    }

    fn policy(&self, call: &AgentCall, _: &Launch<'_>) -> Result<Policy, AgentError> {
        Ok(fenced(
            call,
            &Files::of(&call.settings),
            Path::new(&self.program),
            &self.unfenced_no_read,
        ))
    }

    fn parse(
        &self,
        call: &AgentCall,
        before: Spent,
        output: &Output,
    ) -> Result<AgentReply, AgentError> {
        let id = call.session.id();
        let read = compact::read(call.prompt == COMPACT, output, &call.session, before);
        // The session's totals grow whatever the call's reply was.
        if let Some(spent) = read.spent {
            Files::of(&call.settings).keep(id, spent)?;
        }
        read.reply
    }

    fn follow_up<'a>(&'a self, call: &'a AgentCall) -> Option<FollowUp<'a>> {
        compact::context_of(self, call)
    }

    // The totals the call began with, which `begin` kept before it spawned. A
    // file that is gone is no reason to end a turn that is running: the session
    // counts from nothing, as a new one does.
    fn adopted_begin(&self, adopted: &Adopted) -> Result<Spent, String> {
        let kept = adopted
            .kept
            .as_ref()
            .ok_or_else(|| unserved().to_string())?;
        let before = Files::beside(&adopted.record).spent(&kept.session);
        before
            .map(Option::unwrap_or_default)
            .map_err(|e| e.to_string())
    }

    fn parse_adopted(
        &self,
        adopted: &Adopted,
        before: Spent,
        output: &Output,
    ) -> Result<AgentReply, AgentError> {
        let kept = adopted.kept.as_ref().ok_or_else(unserved)?;
        let session = match &adopted.resumes {
            Some(id) => Session::Resume(id.clone()),
            None => Session::New(kept.session.clone()),
        };
        let compacting = adopted.flying.as_ref().is_some_and(|f| f.compact);
        let read = compact::read(compacting, output, &session, before);
        // The session's totals grow whatever the call's reply was.
        if let Some(spent) = read.spent {
            Files::beside(&adopted.record).keep(&kept.session, spent)?;
        }
        read.reply
    }

    // The terminal session's requests are in goose's database, so its totals hold
    // tokens kelpie cannot count. The record notes that at the start, since the
    // hold outlives a restarted runner. The next headless turn counts from the
    // totals it reports.
    fn started(&self, call: &AgentCall, known: &Spent) -> Result<(), AgentError> {
        let rebased = Spent {
            rebase: true,
            ..*known
        };
        Files::of(&call.settings).keep(call.session.id(), rebased)
    }

    // goose keeps its sessions in SQLite, which cannot be tailed. The call's
    // stdout file is what the watch reads, and it is named for the settings
    // path, so the folder returned is the call's goose root: the stdout file
    // is its sibling under the same stem.
    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        Some(Transcript {
            folder: Files::of(&call.settings).root,
            threads: None,
        })
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        activity::of_stdout(&CallFiles::of(call).stdout)
    }
}

fn unserved() -> AgentError {
    AgentError::Setup("kelpie routed a call to goose that names no model server".into())
}

/// The server the call's model is on, which every goose call needs
fn server(call: &AgentCall) -> Result<&ModelServer, AgentError> {
    call.harness.server().ok_or_else(unserved)
}

// What goose cannot give a call. The agent files already refuse a role goose
// does not take, so this is the second line.
// parity: harness.issue-writer, harness.pm
fn refuse_unsupported(call: &AgentCall) -> Result<(), AgentError> {
    let refused = match (call.role, call.tools) {
        (Role::Researcher, _) => None,
        // The representative reads and runs nothing: with no fence, goose offers it no tool.
        (Role::Representative, Tools::Answer) => None,
        // ADR 0006: a harness that cannot fence cannot be a worker.
        (Role::Worker, Tools::Work) if call.reach.fence.is_none() => Some(
            "goose cannot run a worker outside the kelpie fence: the plugin that holds its \
             guard is written from the fence"
                .to_owned(),
        ),
        // A retro, a reviewer, a triager and an advisor with no fence have no shell and read what
        // kelpie wrote them, as a researcher does.
        (Role::Worker, Tools::Work | Tools::Retro)
        | (Role::Reviewer | Role::Triager, Tools::Review)
        | (Role::Advisor, Tools::Advise) => None,
        (
            Role::Worker | Role::Reviewer | Role::Advisor | Role::Representative | Role::Triager,
            tools,
        ) => Some(format!(
            "goose cannot run {:?} tools for {}",
            tools,
            call.role.as_str()
        )),
        (Role::IssueWriter | Role::Pm, _) => Some(format!(
            "goose cannot run the {}: it runs on Claude Code alone",
            call.role.as_str()
        )),
    };
    if let Some(why) = refused {
        return Err(AgentError::Setup(why));
    }
    // goose discovers these from the working folder, and `settings.local.json`
    // there can disable any plugin by name.
    for file in PROJECT_FILES {
        let path = call.cwd.join(file);
        if path.exists() {
            return Err(AgentError::Setup(format!(
                "goose would load {} whatever `--no-profile` says: move it out of the working folder",
                path.display()
            )));
        }
    }
    Ok(())
}

// Kelpie starts a step's prompt with Claude Code's `/<plugin>:<skill>`. goose
// runs a skill it is given as `/<name> <rest>`, so a prompt that names one of
// the call's skills is that command ([`skills::prompt`]), or the skill's text
// where the call has no tool to load it with.
fn prompt(call: &AgentCall) -> String {
    let offers_tools = !chat(call, &Files::of(&call.settings));
    skills::prompt(call, offers_tools).unwrap_or_else(|| call.prompt.clone())
}

// goose takes its system prompt as text, not as a path.
fn system_text(call: &AgentCall) -> Result<Option<String>, AgentError> {
    let Some(path) = &call.instructions else {
        return Ok(None);
    };
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|e| AgentError::Setup(format!("cannot read {}: {}", path.display(), e.kind())))
}

// Whether `call` is a worker's with its tools and a fence, which goose runs
// behind the fence's hook plugin. A worker with no fence is refused by
// `prepare`, and a command built for one anyway is a researcher's, which has
// no tool.
fn works(call: &AgentCall) -> bool {
    call.role == Role::Worker && call.tools == Tools::Work && call.reach.fence.is_some()
}

// Whether `call` reads the repo through the read-only shell: a reviewer, an
// advisor or a retro with a fence to hold it. One with no fence is offered no
// tool. The researcher is never one, whatever tools it is paired with.
fn reads(call: &AgentCall) -> bool {
    call.reach.fence.is_some()
        && matches!(
            (call.role, call.tools),
            (Role::Worker, Tools::Retro)
                | (Role::Reviewer | Role::Triager, Tools::Review)
                | (Role::Advisor, Tools::Advise)
        )
}

// What the call's hook lets it do. A reviewer whose fence has a folder to write was
// asked for tests, and writes that folder alone. A retro's and an advisor's fence
// can write too, which their hook must not use.
fn judging<'a>(call: &AgentCall, fence: &'a crate::ports::Fence) -> hooks::Judging<'a> {
    match (reads(call), call.tools, fence.write.first()) {
        (false, _, _) => hooks::Judging::Work,
        (true, Tools::Review, Some(folder)) => hooks::Judging::Tests(folder),
        (true, _, _) => hooks::Judging::Reading,
    }
}

// Whether the call runs in `GOOSE_MODE=chat`: it has no shell and its session
// holds no MCP server, so goose rejects any tool call it makes.
fn chat(call: &AgentCall, files: &Files) -> bool {
    !(has_shell(call) || mcp::loads(call, files))
}

// Whether goose offers the call its `developer` tools, behind the hook plugin.
fn has_shell(call: &AgentCall) -> bool {
    works(call) || reads(call)
}

// Whether the call is given `--with-builtin developer`. goose restores a
// resumed session's own extension set, so a resumed call that only reads takes
// none, and the plugin's hook is its only gate on the tools it has. A resumed
// worker is given the builtin again: goose keeps one.
fn offers_developer(call: &AgentCall) -> bool {
    works(call) || (reads(call) && matches!(call.session, Session::New(_)))
}

// `prompt` for a reviewer, with what its shell is for. Its prompt sends it to
// Read, Glob and Grep, which goose lacks. An advisor's and a retro's name none.
fn told(call: &AgentCall, prompt: String) -> String {
    if call.tools != Tools::Review || !reads(call) {
        return prompt;
    }
    let commands: Vec<String> = READ_ONLY_COMMANDS
        .iter()
        .map(|c| format!("`{c}`"))
        .collect();
    format!(
        "{prompt}\n\nThis session has no Read, Glob or Grep tool: read the repo with its `shell` \
         and `tree` instead. The shell runs only {}, each as plain words, and pipes between \
         them. It refuses a redirect, a `$` or a backtick, and any other command, git included. \
         Where the instructions above say to run no command, they mean no other.",
        commands.join(", ")
    )
}

// The variables that move goose into the call's own root and point it at the
// forwarder. `GOOSE_MODE` is `chat` with no extension, else `auto`, where the
// hook plugin is the only gate. A call with a shell reads `AGENTS.md` and
// `CLAUDE.md`, never `.goosehints`. A `mode` replaces `GOOSE_MODE`.
fn env_args(
    call: &AgentCall,
    server: &ModelServer,
    files: &Files,
    upstream: &Upstream,
    mode: Option<&str>,
) -> Vec<OsString> {
    let mut vars: Vec<(String, OsString)> = vec![
        ("GOOSE_PATH_ROOT".into(), files.root.clone().into()),
        ("GOOSE_PROVIDER".into(), "openai".into()),
        ("GOOSE_MODEL".into(), call.model.as_str().into()),
        (
            "GOOSE_CONTEXT_LIMIT".into(),
            server.context.get().to_string().into(),
        ),
        (
            "GOOSE_MODE".into(),
            match mode {
                Some(mode) => mode,
                None if chat(call, files) => "chat",
                None => "auto",
            }
            .into(),
        ),
        ("OPENAI_BASE_URL".into(), upstream.worker_url().into()),
        ("OPENAI_API_KEY".into(), PLACEHOLDER_KEY.into()),
        ("GOOSE_DISABLE_KEYRING".into(), "1".into()),
        ("GOOSE_TELEMETRY_OFF".into(), "1".into()),
        ("TMPDIR".into(), files.scratch.clone().into()),
    ];
    if has_shell(call) {
        vars.push((
            "CONTEXT_FILE_NAMES".into(),
            serde_json::json!(CONTEXT_FILES).to_string().into(),
        ));
    }
    vars.extend(web::env(call));
    vars.extend(compact::threshold_var(call, server.context.get()));
    // The fence adds its variables, and none replaces one that closes a door.
    if let Some(fence) = &call.reach.fence {
        let own: Vec<String> = vars.iter().map(|(name, _)| name.clone()).collect();
        let added = (fence.env.iter()).filter(|(name, _)| !own.contains(name));
        vars.extend(added.map(|(name, value)| (name.clone(), value.clone().into())));
    }
    assignments(vars).collect()
}

// The prompt is one `--text=` argument, so one starting with `-` is not a flag.
// A worker gets the `developer` builtin on a resumed call too, since goose
// de-duplicates a builtin. A reviewer, advisor or retro gets it on a new
// session ([`offers_developer`]), and only a new session gets its servers.
fn argv(call: &AgentCall) -> Result<Vec<OsString>, AgentError> {
    if let Some(compact) = compact::argv(call) {
        return Ok(compact);
    }
    let mut argv: Vec<OsString> = vec![
        "run".into(),
        "--no-profile".into(),
        "--output-format".into(),
        "stream-json".into(),
        format!("--text={}", told(call, prompt(call))).into(),
    ];
    if offers_developer(call) {
        argv.extend(["--with-builtin".into(), "developer".into()]);
    }
    argv.extend(mcp::flags(call));
    // Every call lists its plugins' skills, a resumed one too: goose keeps a builtin once.
    argv.extend(["--with-builtin".into(), "skills".into()]);
    // goose builds its system prompt afresh on every call, a resumed one too.
    if let Some(text) = system_text(call)? {
        argv.push(format!("--system={text}").into());
    }
    // goose keeps a name as given, and `--resume --name` finds the session by it.
    if matches!(call.session, Session::Resume(_)) {
        argv.push("--resume".into());
    }
    argv.extend(["--name".into(), call.session.id().0.as_str().into()]);
    Ok(argv)
}

/// The whole call's policy: its fence, or none, and what goose itself needs
///
/// The model's host is not in it. The one host goose may dial is the forwarder's,
/// which the adapter adds.
fn fenced(call: &AgentCall, files: &Files, program: &Path, unfenced: &[String]) -> Policy {
    let mut policy = call_policy(call, unfenced);
    // A retro and an advisor run under a fence that can write, which their
    // read-only shell must not use: the hook is the first layer, this the second.
    if matches!(call.tools, Tools::Retro | Tools::Advise) {
        policy.write.clear();
    }
    policy.no_keychain = call.reach.block_keychain;
    skills::fence(&mut policy, files);
    // The call logs in nowhere and goose's config is relocated, so no harness's
    // login is its own, the maintainer's goose included.
    let logins = fence::HARNESSES.iter().flat_map(|own| own.credentials);
    policy.no_read.extend(logins.map(|&path| path.to_owned()));
    policy
        .write
        .extend([files.data(), files.state(), files.scratch.clone()]);
    let outputs = CallFiles::of(call);
    policy.read.extend(
        [files.root.clone(), files.scratch.clone()]
            .into_iter()
            .chain(program_folder(program, &ProcessEnv, &files.root))
            .chain([outputs.stdout, outputs.stderr])
            .chain(call.instructions.iter().cloned())
            .chain(call.plugin_dirs.iter().cloned())
            .chain(call.reach.read.iter().cloned()),
    );
    bind_in_scratch(&mut policy, call, &files.scratch);
    crate::adapters::mcp::fence_the_graph(&mut policy, call);
    crate::adapters::mcp::fence_the_browser(&mut policy, call);
    web::open(call, &mut policy);
    policy
}

#[cfg(test)]
fn policy(
    call: &AgentCall,
    files: &Files,
    upstream: &Upstream,
    forwarder: &Path,
    program: &Path,
    unfenced: &[String],
) -> Policy {
    let mut policy = fenced(call, files, program, unfenced);
    super::harness::dial_the_forwarder(&mut policy, upstream, forwarder);
    policy
}
