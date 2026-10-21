//! Headless Claude Code, one `claude -p` process per call

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde::Deserialize;

use super::harness::{Base, adoption_of, prompt_file};
use super::process::RunError;
use crate::ports::{
    Adopted, AgentCall, AgentError, AgentReply, Agents, CallActivity, Cost, Ending, Role, Session,
    SessionId, Transcript, Usage, written_at,
};
use crate::settings::Harness;

/// This adapter's harness, as its errors name it
const CLAUDE: Harness = Harness::ClaudeCode;

mod adopt;
#[cfg(test)]
mod calls;
#[cfg(test)]
mod escapes;
mod graph;
mod mcp;
pub(crate) mod sandbox;
pub(crate) mod settings;

/// What `claude -p --resume` prints when the session has no transcript
const NO_SESSION: &str = "No conversation found with session ID";

/// How to give `claude` the login it needs where the keychain is blocked, as doctor and a refused call say it
pub const LOGIN_FIX: &str = "run `claude setup-token`, then `shep secret set CLAUDE_CODE_OAUTH_TOKEN` and set \
     `CLAUDE_CODE_OAUTH_TOKEN = \"{{secret:CLAUDE_CODE_OAUTH_TOKEN}}\"` in the runner's `env`";

/// The variable Claude Code reads a login from, in place of the keychain's
pub const LOGIN_VAR: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// The login `claude setup-token` makes: one token, not the keychain's full login
///
/// Where a call's sandbox blocks the keychain, `claude` cannot read its own
/// login there. The maintainer keeps this one with `shep secret`, and a runner
/// hands it to `claude` alone.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginToken(String);

impl LoginToken {
    /// The token `value` is, or `None` when it is empty
    pub fn new(value: String) -> Option<Self> {
        (!value.trim().is_empty()).then(|| Self(value.trim().to_owned()))
    }

    /// Takes the token from this process's environment, and keeps the variable
    /// out of every process started from now on, `claude`'s apart
    pub fn take() -> Option<Self> {
        let token = std::env::var(LOGIN_VAR).ok().and_then(Self::new);
        crate::spawn::hide([LOGIN_VAR.to_owned()]);
        token
    }

    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LoginToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginToken(..)")
    }
}

/// Headless Claude Code, one `claude -p` process per call, each inside the sandbox
///
/// Clones share their calls in flight, so one clone can stop them all.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub(super) base: Base,
    program: OsString,
    // The login `claude` runs with, in place of the maintainer's in the keychain
    login: Option<LoginToken>,
    // Whether this is macOS, where a blocked keychain takes `claude`'s own login with it
    macos: bool,
}

impl Default for ClaudeCli {
    fn default() -> Self {
        Self::on(&Base::default())
    }
}

impl ClaudeCli {
    /// Claude Code on `base`: its sandbox, lamb labels and calls in flight
    pub fn on(base: &Base) -> Self {
        Self {
            base: base.clone(),
            program: "claude".into(),
            login: None,
            macos: cfg!(target_os = "macos"),
        }
    }

    /// Runs `program` in place of `claude`, as a stand-in script does
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_program(self, program: PathBuf) -> Self {
        Self {
            program: program.into(),
            ..self
        }
    }

    /// Runs `claude` with `login` in its environment, where there is one, in
    /// place of the keychain's login
    #[must_use]
    pub fn with_login(self, login: Option<LoginToken>) -> Self {
        Self { login, ..self }
    }

    /// The same, as on macOS or not, which a test runs on either
    #[cfg(test)]
    pub(crate) fn on_macos(self, macos: bool) -> Self {
        Self { macos, ..self }
    }
}

impl Agents for ClaudeCli {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        write_settings(call, &self.base.unfenced_no_read)?;
        sandbox::policy(call, &self.base.home, &self.base.unfenced_no_read).map(drop)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let mut command = self.sandboxed_command(call)?;
        let label = call.label();
        let spawned = |pid| {
            if let Some(lambs) = &self.base.lambs {
                lambs.label(pid, &label);
            }
        };
        let run = (self.base.processes).output_of_call(&mut command, call, ending, &spawned);
        let output = run.map_err(|e| match e {
            RunError::Io(e) => AgentError::Spawn(CLAUDE, e.to_string()),
            RunError::Stopped => AgentError::Stopped,
            RunError::TimedOut => AgentError::TimedOut(CLAUDE),
        })?;
        let mut reply = parse_result(&output, &call.session)?;
        reply.compactions = compactions(call);
        Ok(reply)
    }

    fn adoption(&self, call: &AgentCall) -> Result<Adopted, String> {
        adoption_of(call, Harness::ClaudeCode)
    }

    fn adopt(&self, adopted: &Adopted, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.adopt_call(adopted, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        let folder = sandbox::transcripts(&self.base.home, &call.cwd).ok()?;
        Some(Transcript {
            folder,
            threads: None,
        })
    }

    // Claude Code appends to the session's transcript at each message and
    // tool call, so a write is the session doing something.
    fn last_active(&self, call: &AgentCall) -> CallActivity {
        let Ok(folder) = sandbox::transcripts(&self.base.home, &call.cwd) else {
            return CallActivity::Untracked;
        };
        written_at(&folder.join(format!("{}.jsonl", call.session.id().0)))
    }

    fn foreground(&self, call: &AgentCall) -> Result<Command, AgentError> {
        self.terminal_command(call)
    }
}

/// Writes the call's settings file whole, from the call alone
pub fn write_settings(call: &AgentCall, unfenced: &[String]) -> Result<(), AgentError> {
    let mut value = settings::settings(call.tools, &call.reach);
    // The prompt beside the settings file, for the hook that hands it back.
    let again = prompt_file(&call.settings);
    if let Some(window) = call.compact_window {
        let marks = settings::compactions_file(&sandbox::scratch(call));
        settings::compacting(&mut value, window, &again, &marks);
    }
    mcp::allow(&mut value, call);
    let reads = settings::unfenced_reads(&call.cwd, &call.reach, unfenced);
    if let Some(deny) = value["permissions"]["deny"].as_array_mut() {
        deny.extend(reads.into_iter().map(serde_json::Value::from));
    }
    let text = serde_json::to_string_pretty(&value).expect("settings are JSON");
    let folder = call.settings.parent().unwrap_or(std::path::Path::new("/"));
    let write = |path: &std::path::Path, text: String| {
        std::fs::write(path, text).map_err(|e| {
            AgentError::Setup(format!("cannot write {}: {}", path.display(), e.kind()))
        })
    };
    std::fs::create_dir_all(folder).map_err(|e| {
        AgentError::Setup(format!(
            "cannot write {}: {}",
            call.settings.display(),
            e.kind()
        ))
    })?;
    write(&call.settings, text)?;
    if let Some(config) = mcp::config(call) {
        let text = serde_json::to_string_pretty(&config).expect("an MCP config is JSON");
        write(&mcp::mcp_file(&call.settings), text)?;
    }
    match call.compact_window {
        Some(_) => write(&again, compacted_prompt(&call.prompt)),
        None => Ok(()),
    }
}

// What a compaction's hook prints into the session: the turn's prompt again.
fn compacted_prompt(prompt: &str) -> String {
    format!(
        "Claude Code compacted your session partway through this turn, so what came before is \
         a summary, and the prompt may have been cut from it. This is the prompt again:\n\n\
         {prompt}\n"
    )
}

// `--setting-sources project` keeps the project's CLAUDE.md and skills and
// drops the maintainer's own hooks, plugins and skills. It also drops the
// worktree's `settings.local.json`, which nothing kelpie runs needs.
fn argv(call: &AgentCall) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec!["-p".into(), call.prompt.as_str().into()];
    argv.extend(profile(call));
    argv.extend(["--output-format".into(), "json".into()]);
    // Only a worker's working call is let edit; a retro changes nothing.
    if call.role == Role::Worker && call.tools == crate::ports::Tools::Work {
        argv.extend(["--permission-mode".into(), "bypassPermissions".into()]);
    }
    argv.extend(session(call));
    argv
}

// The same session in the maintainer's terminal: Claude Code asks before
// each tool call as it always does, and a prompt is its first message.
fn terminal_argv(call: &AgentCall) -> Vec<OsString> {
    let mut argv = profile(call);
    argv.extend(session(call));
    if !call.prompt.is_empty() {
        argv.push(call.prompt.as_str().into());
    }
    argv
}

fn profile(call: &AgentCall) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec![
        "--model".into(),
        call.model.as_str().into(),
        "--effort".into(),
        call.effort.as_str().into(),
        "--setting-sources".into(),
        "project".into(),
        "--settings".into(),
        call.settings.clone().into(),
    ];
    // Every kind of call but an answer's runs on an allow-list. Every one but
    // the issue writer's loads no MCP server but those kelpie names, so what
    // a project's settings allow adds nothing. An agent file's own list
    // replaces a worker's and a reviewer's, and no other's.
    use crate::ports::Tools as Kind;
    let own = call.agent_tools.as_deref();
    let tools = match (call.role, call.tools) {
        (_, Kind::Retro) => Some(settings::RETRO_TOOLS),
        (Role::Pm, _) => Some(settings::PM_TOOLS),
        (_, Kind::Advise) => Some(settings::ADVISE_TOOLS),
        (_, Kind::Research) => Some(settings::RESEARCH_TOOLS),
        (_, Kind::Work) => Some(own.unwrap_or(settings::WORK_TOOLS)),
        (_, Kind::Review) => Some(own.unwrap_or(crate::settings::CLAUDE_REVIEW_TOOLS)),
        // Its call loads the repo's plugins and their servers on purpose, so
        // it is allow-listed without `--strict-mcp-config`.
        (_, Kind::Issues) => Some(settings::ISSUES_TOOLS),
        (_, Kind::Answer | Kind::Pm) => None,
    };
    if crate::ports::mcp::loads(call) {
        argv.extend(["--mcp-config".into(), mcp::mcp_file(&call.settings).into()]);
    }
    if let Some(tools) = tools {
        argv.extend(["--tools".into(), tools.into()]);
        // Only the config named above is loaded, so its servers are the call's MCP
        // servers. The issue writer is the one call that keeps the other sources.
        if call.tools != Kind::Issues {
            argv.push("--strict-mcp-config".into());
        }
    }
    argv
}

fn session(call: &AgentCall) -> Vec<OsString> {
    let mut argv = Vec::new();
    for plugin in &call.plugin_dirs {
        argv.extend(["--plugin-dir".into(), plugin.into()]);
    }
    match &call.session {
        Session::New(id) => {
            argv.extend(["--session-id".into(), id.0.as_str().into()]);
            if let Some(instructions) = &call.instructions {
                argv.extend([
                    "--append-system-prompt-file".into(),
                    instructions.clone().into(),
                ]);
            }
        }
        Session::Resume(id) => argv.extend(["--resume".into(), id.0.as_str().into()]),
    }
    argv
}

// `claude -p` exits 1 on an error result but still prints its JSON, so the
// JSON is read before the exit status.
fn parse_result(output: &Output, session: &Session) -> Result<AgentReply, AgentError> {
    #[derive(Deserialize)]
    struct ResultMessage {
        is_error: bool,
        session_id: String,
        #[serde(default)]
        result: String,
        #[serde(default)]
        usage: Option<ResultUsage>,
        total_cost_usd: Option<f64>,
    }
    #[derive(Deserialize)]
    struct ResultUsage {
        input_tokens: u64,
        cache_creation_input_tokens: u64,
        cache_read_input_tokens: u64,
        output_tokens: u64,
        // The cache writes split by how long they are kept, where reported
        #[serde(default)]
        cache_creation: Option<CacheCreation>,
        // One entry per model request, the last one's input being the context
        #[serde(default)]
        iterations: Vec<Iteration>,
    }
    #[derive(Deserialize)]
    struct CacheCreation {
        #[serde(default)]
        ephemeral_5m_input_tokens: u64,
    }
    #[derive(Deserialize)]
    struct Iteration {
        input_tokens: u64,
        cache_creation_input_tokens: u64,
        cache_read_input_tokens: u64,
    }
    let stdout = || String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let r = match serde_json::from_slice::<ResultMessage>(&output.stdout) {
        Ok(r) if !r.is_error && output.status.success() => r,
        Ok(r) => return Err(AgentError::Failed(CLAUDE, r.result)),
        Err(_) if output.status.success() => return Err(AgentError::Unreadable(CLAUDE, stdout())),
        Err(_) => {
            return Err(match session {
                Session::Resume(id) if stderr.contains(NO_SESSION) => {
                    AgentError::NoSession(CLAUDE, id.clone())
                }
                _ => AgentError::Failed(CLAUDE, stderr.into_owned()),
            });
        }
    };
    let (Some(u), Some(cost)) = (r.usage, r.total_cost_usd.and_then(Cost::from_usd)) else {
        return Err(AgentError::Unreadable(CLAUDE, stdout()));
    };
    let context = (u.iterations.last()).map(|last| {
        last.input_tokens + last.cache_creation_input_tokens + last.cache_read_input_tokens
    });
    Ok(AgentReply {
        session_id: SessionId(r.session_id),
        text: r.result,
        usage: Usage {
            input: u.input_tokens,
            cache_write: u.cache_creation_input_tokens,
            cache_write_5m: (u.cache_creation.map_or(0, |c| c.ephemeral_5m_input_tokens))
                .min(u.cache_creation_input_tokens),
            cache_read: u.cache_read_input_tokens,
            output: u.output_tokens,
        },
        session_cost: Some(cost),
        context,
        compactions: 0,
    })
}

// The compactions Claude Code did in the call: one mark for each, left by the hook that hands
// the prompt back. A call another image adopted is not counted, its marks out of reach.
fn compactions(call: &AgentCall) -> u32 {
    let marks = settings::compactions_file(&sandbox::scratch(call));
    let len = std::fs::metadata(marks).map_or(0, |m| m.len());
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
