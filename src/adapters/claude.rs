//! Headless Claude Code, one `claude -p` process per call

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use serde::Deserialize;

use super::process::{Processes, RunError};
use super::srt::SandboxRuntime;
use crate::ports::{
    Adopted, AgentCall, AgentError, AgentReply, Agents, CallActivity, Cost, Ending, Role, Sandbox,
    Session, SessionId, Transcript, Unreadable, Usage, written_at,
};
use crate::settings::Harness;
use crate::tools::Tools;

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

/// Names a runner's child process in `shep describe`
pub trait LambLabels: Send + Sync + fmt::Debug {
    /// Labels the child `pid` with `label`
    fn label(&self, pid: u32, label: &str);
}

impl LambLabels for shep_channel::Shepherd {
    // A label kelpie builds is short and plain, so a refused one is dropped
    // rather than failing the call it names.
    fn label(&self, pid: u32, label: &str) {
        if let Ok(label) = shep_channel::LambLabel::new(label) {
            let _ = self.label_lamb(pid, label);
        }
    }
}

/// Headless Claude Code, one `claude -p` process per call, each inside the sandbox
///
/// Clones share their calls in flight, so one clone can stop them all.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub(super) processes: Processes,
    program: OsString,
    pub(super) lambs: Option<Arc<dyn LambLabels>>,
    pub(super) sandbox: Arc<dyn Sandbox>,
    pub(super) home: PathBuf,
    // What a call with no fence may not read either: the shepherd's home
    pub(super) unfenced_no_read: Vec<String>,
    // The login `claude` runs with, in place of the maintainer's in the keychain
    login: Option<LoginToken>,
    // Whether this is macOS, where a blocked keychain takes `claude`'s own login with it
    macos: bool,
}

// The default home and tools are the maintainer's: `$HOME` and kelpie's own tools.
impl Default for ClaudeCli {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            processes: Processes::default(),
            program: "claude".into(),
            lambs: None,
            sandbox: Arc::new(SandboxRuntime::new(Tools::under(
                &crate::home::kelpie_home().unwrap_or_else(|_| home.join(crate::home::OLD)),
            ))),
            home,
            unfenced_no_read: Vec::new(),
            login: None,
            macos: cfg!(target_os = "macos"),
        }
    }
}

impl ClaudeCli {
    /// Labels each call's process with its issue and role, such as `#7 worker`
    pub fn labelling(lambs: Arc<dyn LambLabels>) -> Self {
        Self {
            lambs: Some(lambs),
            ..Self::default()
        }
    }

    /// Runs `program` in place of `claude`, as a stand-in script does
    #[cfg(test)]
    pub(crate) fn with_program(program: PathBuf) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    /// Runs each call inside the sandbox runtime in `tools`, as Claude Code
    /// with its home at `home`, the way a runner does
    ///
    /// No call on any harness reads kelpie's Codex login in `codex_home`,
    /// apart from Codex's own link to its `auth.json`, or sees the variables
    /// in `secret_vars`. pi and Codex made from this share its sandbox.
    pub fn in_runtime(
        self,
        tools: Tools,
        home: PathBuf,
        codex_home: &Path,
        secret_vars: Vec<String>,
    ) -> Self {
        let runtime = Arc::new(SandboxRuntime::new(tools));
        let unread = vec![format!("{}/**", codex_home.display())];
        let sandbox = Unreadable::new(runtime, unread).unsetting(secret_vars);
        self.sandboxed(Arc::new(sandbox), home)
    }

    /// Runs `claude` with `login` in its environment, where there is one, in
    /// place of the keychain's login
    ///
    /// pi and Codex made from this get none of it.
    pub fn with_login(self, login: Option<LoginToken>) -> Self {
        Self { login, ..self }
    }

    /// The same, as on macOS or not, which a test runs on either
    #[cfg(test)]
    pub(crate) fn on_macos(self, macos: bool) -> Self {
        Self { macos, ..self }
    }

    /// Keeps a call with no fence, such as a reviewer's session, out of
    /// `shep_home`, which holds the sheep entries' `env` and `dogs.toml`
    ///
    /// pi and Codex made from this after share it.
    pub fn unfenced_unread(mut self, shep_home: &Path) -> Self {
        self.unfenced_no_read = vec![format!("{}/**", shep_home.display())];
        self
    }

    /// Runs each call inside `sandbox`, as Claude Code with its home at `home`
    pub fn sandboxed(self, sandbox: Arc<dyn Sandbox>, home: PathBuf) -> Self {
        Self {
            sandbox,
            home,
            ..self
        }
    }

    /// Refuses new calls as the runner stops, sends those in flight SIGTERM,
    /// and leaves the rest to shep's stop
    ///
    /// A call refused, or one that ends after, returns [`AgentError::Stopped`].
    pub fn stop(&self) {
        self.processes.stop();
    }
}

impl Agents for ClaudeCli {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        write_settings(call, &self.unfenced_no_read)?;
        sandbox::policy(call, &self.home, &self.unfenced_no_read).map(drop)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let mut command = self.sandboxed_command(call)?;
        let label = call.label();
        let spawned = |pid| {
            if let Some(lambs) = &self.lambs {
                lambs.label(pid, &label);
            }
        };
        let run = self
            .processes
            .output_of_call(&mut command, call, ending, &spawned);
        let output = run.map_err(|e| match e {
            RunError::Io(e) => AgentError::Spawn(CLAUDE, e.to_string()),
            RunError::Stopped => AgentError::Stopped,
            RunError::TimedOut => AgentError::TimedOut(CLAUDE),
        })?;
        parse_result(&output, &call.session)
    }

    fn adoption(&self, call: &AgentCall) -> Result<Adopted, String> {
        Self::adoption_of(call)
    }

    fn adopt(&self, adopted: &Adopted, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.adopt_call(adopted, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        let folder = sandbox::transcripts(&self.home, &call.cwd).ok()?;
        Some(Transcript {
            folder,
            threads: None,
        })
    }

    // Claude Code appends to the session's transcript at each message and
    // tool call, so a write is the session doing something.
    fn last_active(&self, call: &AgentCall) -> CallActivity {
        let Ok(folder) = sandbox::transcripts(&self.home, &call.cwd) else {
            return CallActivity::Untracked;
        };
        written_at(&folder.join(format!("{}.jsonl", call.session.id().0)))
    }

    fn foreground(&self, call: &AgentCall) -> Result<Command, AgentError> {
        self.terminal_command(call)
    }
}

/// Writes the call's settings file whole, from the call alone
pub(crate) fn write_settings(call: &AgentCall, unfenced: &[String]) -> Result<(), AgentError> {
    let mut value = settings::settings(call.tools, &call.reach);
    // The prompt beside the settings file, for the hook that hands it back.
    let again = settings::prompt_file(&call.settings);
    if let Some(window) = call.compact_window {
        settings::compacting(&mut value, window, &again);
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
    // Every kind of call but an answer or the issue writer's runs on an
    // allow-list and loads no MCP server but those kelpie names, so what a
    // project's settings allow adds nothing. An agent file's own list replaces a worker's and a
    // reviewer's, and no other's.
    use crate::ports::Tools as Kind;
    let own = call.agent_tools.as_deref();
    let tools = match (call.role, call.tools) {
        (_, Kind::Retro) => Some(settings::RETRO_TOOLS),
        (Role::Pm, _) => Some(settings::PM_TOOLS),
        (_, Kind::Advise) => Some(settings::ADVISE_TOOLS),
        (_, Kind::Research) => Some(settings::RESEARCH_TOOLS),
        (_, Kind::Work) => Some(own.unwrap_or(settings::WORK_TOOLS)),
        (_, Kind::Review) => Some(own.unwrap_or(settings::REVIEW_TOOLS)),
        (_, Kind::Answer | Kind::Issues | Kind::Pm) => None,
    };
    if crate::adapters::mcp::loads(call) {
        argv.extend(["--mcp-config".into(), mcp::mcp_file(&call.settings).into()]);
    }
    if let Some(tools) = tools {
        // Only the config named above is loaded, so its servers are the call's MCP servers.
        argv.extend(["--tools".into(), tools.into(), "--strict-mcp-config".into()]);
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
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;
    use crate::ports::Tools;
    use crate::settings::Effort;
    use crate::test::OpenSandbox;

    #[test]
    fn a_runners_calls_on_every_harness_leave_kelpies_codex_login_unread() {
        let dir = tempfile::tempdir().unwrap();
        let tools = crate::tools::Tools::at(dir.path().join("tools"));
        std::fs::create_dir_all(tools.sandbox().parent().unwrap()).unwrap();
        std::fs::write(tools.sandbox(), "").unwrap();
        let login = dir.path().join("codex");
        let claude =
            ClaudeCli::default().in_runtime(tools, dir.path().join("home"), &login, Vec::new());
        let settings = dir.path().join("call.srt.json");
        claude
            .sandbox
            .wrap(
                &crate::ports::Policy::default(),
                &settings,
                &std::process::Command::new("claude"),
            )
            .unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let unread = format!("{}/**", login.display());
        assert!(
            written["filesystem"]["denyRead"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == unread.as_str()),
            "{written}"
        );
    }

    #[test]
    fn a_calls_activity_is_when_its_session_transcript_was_last_written() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let claude = ClaudeCli::default().sandboxed(Arc::new(OpenSandbox::default()), home.clone());
        let mut call = call(Role::Worker, fresh());
        call.cwd = dir.path().join("wt");
        std::fs::create_dir_all(&call.cwd).unwrap();
        assert_eq!(
            claude.last_active(&call),
            CallActivity::Nothing,
            "no transcript yet"
        );

        let folder = sandbox::transcripts(&home, &call.cwd).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let transcript = folder.join(format!("{}.jsonl", call.session.id().0));
        std::fs::write(&transcript, "{\"type\":\"assistant\"}\n").unwrap();
        assert_eq!(
            claude.last_active(&call),
            crate::ports::written_at(&transcript)
        );
        assert!(matches!(claude.last_active(&call), CallActivity::At(_)));
    }

    #[test]
    fn a_calls_transcript_is_in_the_folder_claude_code_names_for_its_working_folder() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let claude = ClaudeCli::default().sandboxed(Arc::new(OpenSandbox::default()), home.clone());
        let mut call = call(Role::Worker, fresh());
        call.cwd = dir.path().join("wt");
        std::fs::create_dir_all(&call.cwd).unwrap();
        assert_eq!(
            claude.transcript(&call),
            Some(crate::ports::Transcript {
                folder: sandbox::transcripts(&home, &call.cwd).unwrap(),
                threads: None,
            })
        );
    }

    // Recorded from Claude Code 2.1.283 on Haiku: a call asked to say ok.
    const RESULT: &str = include_str!("../../fixtures/claude-p-result.json");
    // Recorded from Claude Code 2.1.283 on Haiku: a new session, then the
    // same session resumed for a second call.
    const FRESH: &str = include_str!("../../fixtures/claude-p-fresh.json");
    const RESUMED: &str = include_str!("../../fixtures/claude-p-resumed.json");
    // Recorded: `--resume` onto a session killed before it wrote a transcript.
    const NO_SESSION_STDERR: &str = include_str!("../../fixtures/claude-p-no-session.stderr");

    fn output(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn resume(id: &str) -> Session {
        Session::Resume(SessionId(id.into()))
    }

    fn fresh() -> Session {
        Session::New(SessionId("7e812e8a-3bf1-42a7-bddf-0ab283b7372a".into()))
    }

    fn call(role: Role, session: Session) -> AgentCall {
        AgentCall {
            issue: 6,
            settings: PathBuf::from("/k/worker/settings.json"),
            instructions: Some(PathBuf::from("/k/worker/instructions.md")),
            ..AgentCall::new(
                role,
                Tools::Work,
                crate::settings::AgentHarness::ClaudeCode,
                "claude-sonnet-5",
                Effort::Medium,
                session,
                PathBuf::from("/tmp"),
                "implement #6",
            )
        }
    }

    fn strings(call: &AgentCall) -> Vec<String> {
        argv(call)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn a_new_worker_session_carries_the_whole_profile() {
        assert_eq!(
            strings(&call(Role::Worker, fresh())),
            [
                "-p",
                "implement #6",
                "--model",
                "claude-sonnet-5",
                "--effort",
                "medium",
                "--setting-sources",
                "project",
                "--settings",
                "/k/worker/settings.json",
                "--tools",
                "Bash,Read,Edit,Write,Glob,Grep,Skill",
                "--strict-mcp-config",
                "--output-format",
                "json",
                "--permission-mode",
                "bypassPermissions",
                "--session-id",
                "7e812e8a-3bf1-42a7-bddf-0ab283b7372a",
                "--append-system-prompt-file",
                "/k/worker/instructions.md",
            ]
        );
    }

    #[test]
    fn a_resumed_session_names_it_and_keeps_its_instructions() {
        let argv = strings(&call(Role::Worker, resume("abc")));
        assert_eq!(argv[argv.len() - 2..], ["--resume", "abc"]);
        assert!(!argv.iter().any(|a| a == "--append-system-prompt-file"));
        assert!(!argv.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn a_retro_resumes_the_workers_session_without_bypassing_permissions() {
        let mut retro = call(Role::Worker, resume("abc"));
        retro.tools = crate::ports::Tools::Retro;
        let argv = strings(&retro);
        assert!(!argv.iter().any(|a| a == "bypassPermissions"), "{argv:?}");
        assert_eq!(argv[argv.len() - 2..], ["--resume", "abc"]);
    }

    #[test]
    fn a_retro_runs_on_an_allow_list_of_tools_and_no_mcp_server() {
        for role in [Role::Worker, Role::Pm] {
            let mut retro = call(role, fresh());
            retro.tools = crate::ports::Tools::Retro;
            let argv = strings(&retro);
            let at = argv.iter().position(|a| a == "--tools").unwrap();
            assert_eq!(argv[at + 1], "Read,Glob,Grep,Skill", "{role:?}");
            assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
        }
    }

    #[test]
    fn each_plugin_folder_is_its_own_plugin_dir() {
        let mut with = call(Role::Reviewer, fresh());
        with.plugin_dirs = vec!["/k/skills/mattpocock".into(), "/k/skills/review".into()];
        let argv = strings(&with);
        let dirs: Vec<&str> = argv
            .windows(2)
            .filter(|pair| pair[0] == "--plugin-dir")
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(dirs, ["/k/skills/mattpocock", "/k/skills/review"]);
        let argv = strings(&call(Role::Reviewer, fresh()));
        assert!(!argv.iter().any(|a| a == "--plugin-dir"));
    }

    #[test]
    fn only_a_worker_bypasses_permissions() {
        let argv = strings(&call(Role::Reviewer, fresh()));
        assert!(!argv.iter().any(|a| a == "--permission-mode"));
    }

    #[test]
    fn prepare_writes_the_settings_file_the_call_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut review = call(Role::Reviewer, fresh());
        review.settings = dir.path().join("worker/review-settings.json");
        review.tools = Tools::Review;
        review.reach.read = vec![dir.path().join("extra")];
        ClaudeCli::default().prepare(&review).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&review.settings).unwrap()).unwrap();
        assert_eq!(
            written["permissions"]["deny"],
            crate::trim::deny_with_trim(&["Agent", "Task", "Bash"])
        );
        assert_eq!(
            written["permissions"]["additionalDirectories"],
            serde_json::json!([dir.path().join("extra")])
        );
        assert_eq!(written["disableBundledSkills"], true);

        std::fs::create_dir_all(dir.path().join("taken/settings.json")).unwrap();
        review.settings = dir.path().join("taken/settings.json");
        let err = ClaudeCli::default().prepare(&review).unwrap_err();
        assert!(
            matches!(&err, AgentError::Setup(reason) if reason.starts_with("cannot write ")),
            "{err:?}"
        );
    }

    #[test]
    fn a_success_result_is_read() {
        let reply = parse_result(&output(0, RESULT, ""), &fresh()).unwrap();
        assert_eq!(reply.session_id.0, "f83901c5-6d39-421b-b709-7828b56ba237");
        assert_eq!(reply.text, "ok");
        assert_eq!(
            reply.usage,
            Usage {
                input: 10,
                cache_write: 8003,
                cache_write_5m: 0,
                cache_read: 13673,
                output: 53,
            }
        );
        assert_eq!(reply.session_cost, Some(Cost(17_648_300)));
    }

    // The recorded results cache for an hour only, so the split is moved here.
    #[test]
    fn five_minute_cache_writes_are_kept_apart_from_the_hour_s() {
        let split = RESULT.replace(
            r#""cache_creation":{"ephemeral_1h_input_tokens":8003,"ephemeral_5m_input_tokens":0}"#,
            r#""cache_creation":{"ephemeral_1h_input_tokens":5003,"ephemeral_5m_input_tokens":3000}"#,
        );
        assert_ne!(split, RESULT, "the recorded split moved");
        let reply = parse_result(&output(0, &split, ""), &fresh()).unwrap();
        assert_eq!(
            (reply.usage.cache_write, reply.usage.cache_write_5m),
            (8003, 3000)
        );
        let none = RESULT.replace(r#""cache_creation":{"#, r#""cache_made":{"#);
        let reply = parse_result(&output(0, &none, ""), &fresh()).unwrap();
        assert_eq!(reply.usage.cache_write_5m, 0, "no split is all an hour's");
    }

    #[test]
    fn the_project_manager_runs_on_an_allow_list_of_tools_and_no_mcp_server() {
        for pm in [
            argv(&call(Role::Pm, fresh())),
            terminal_argv(&call(Role::Pm, fresh())),
        ] {
            let at = pm.iter().position(|a| a == "--tools").unwrap();
            assert_eq!(pm[at + 1], "Read,Glob,Grep,Edit,Write");
            assert!(pm.iter().any(|a| a == "--strict-mcp-config"));
            assert!(!pm.iter().any(|a| a == "bypassPermissions"));
        }
    }

    #[test]
    fn the_advisor_and_the_researcher_run_on_allow_lists_of_reading_tools_and_no_mcp_server() {
        let tools = |role, tools| {
            let mut vote = call(role, fresh());
            vote.tools = tools;
            let argv = strings(&vote);
            let at = argv.iter().position(|a| a == "--tools").unwrap();
            assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
            assert!(!argv.iter().any(|a| a == "bypassPermissions"));
            argv[at + 1].clone()
        };
        assert_eq!(tools(Role::Advisor, Tools::Advise), "Read,Glob,Grep");
        assert_eq!(
            tools(Role::Researcher, Tools::Research),
            "Read,Glob,Grep,WebFetch,WebSearch"
        );
    }

    #[test]
    fn a_worker_runs_on_the_tools_its_skills_use_and_no_mcp_server() {
        for worker in [
            strings(&call(Role::Worker, fresh())),
            strings(&call(Role::Worker, resume("abc"))),
        ] {
            let at = worker.iter().position(|a| a == "--tools").unwrap();
            assert_eq!(worker[at + 1], "Bash,Read,Edit,Write,Glob,Grep,Skill");
            assert!(worker.iter().any(|a| a == "--strict-mcp-config"));
        }
    }

    #[test]
    fn a_reviewer_runs_on_the_tools_a_read_only_review_uses_and_no_mcp_server() {
        let mut review = call(Role::Reviewer, fresh());
        review.tools = Tools::Review;
        let argv = strings(&review);
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(argv[at + 1], "Read,Glob,Grep");
        assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn a_triager_runs_on_read_glob_grep_and_no_mcp_server() {
        let mut triage = call(Role::Triager, fresh());
        triage.tools = Tools::Review;
        let argv = strings(&triage);
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(argv[at + 1], "Read,Glob,Grep");
        assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
        assert!(!argv.iter().any(|a| a == "bypassPermissions"));
    }

    #[test]
    fn an_agent_files_tools_replace_a_workers_and_a_reviewers_but_no_others() {
        let tools_of = |call: &AgentCall| {
            let argv = strings(call);
            let at = argv.iter().position(|a| a == "--tools")?;
            Some(argv[at + 1].clone())
        };
        let mut worker = call(Role::Worker, fresh());
        worker.agent_tools = Some("Bash,Read,Edit,Write,Glob,Grep,Skill,WebFetch".into());
        assert_eq!(
            tools_of(&worker).as_deref(),
            Some("Bash,Read,Edit,Write,Glob,Grep,Skill,WebFetch")
        );
        let mut review = call(Role::Reviewer, fresh());
        review.tools = Tools::Review;
        review.agent_tools = Some("Read,Glob,Grep,Skill".into());
        assert_eq!(tools_of(&review).as_deref(), Some("Read,Glob,Grep,Skill"));
        // A retro stays read-only.
        worker.tools = Tools::Retro;
        assert_eq!(tools_of(&worker).as_deref(), Some("Read,Glob,Grep,Skill"));
    }

    #[test]
    fn the_context_is_the_last_model_requests_input_not_the_calls_sum() {
        // Eight model requests, whose cache reads sum to 108,226 tokens.
        let many = include_str!("../../fixtures/claude-p-question.json");
        let reply = parse_result(&output(0, many, ""), &fresh()).unwrap();
        assert_eq!(reply.usage.cache_read, 108_226);
        assert_eq!(reply.context, Some(2 + 1_573 + 30_304));
        let one = parse_result(&output(0, RESULT, ""), &fresh()).unwrap();
        assert_eq!(one.context, Some(10 + 8_003 + 13_673));
    }

    #[test]
    fn a_resumed_call_reports_its_own_usage_and_the_session_cost_so_far() {
        let first = parse_result(&output(0, FRESH, ""), &fresh()).unwrap();
        let second = parse_result(&output(0, RESUMED, ""), &resume(&first.session_id.0)).unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert_eq!(
            (first.usage, first.session_cost),
            (
                Usage {
                    input: 10,
                    cache_write: 8984,
                    cache_write_5m: 0,
                    cache_read: 13673,
                    output: 148,
                },
                Some(Cost(20_085_300)),
            )
        );
        assert_eq!(
            (second.usage, second.session_cost),
            (
                Usage {
                    input: 10,
                    cache_write: 193,
                    cache_write_5m: 0,
                    cache_read: 22657,
                    output: 34,
                },
                Some(Cost(22_917_000)),
            )
        );
    }

    #[test]
    fn resuming_a_session_that_never_started_is_named() {
        let err = parse_result(&output(1, "", NO_SESSION_STDERR), &resume("0e2c")).unwrap_err();
        assert_eq!(err, AgentError::NoSession(CLAUDE, SessionId("0e2c".into())));
    }

    #[test]
    fn the_same_stderr_for_a_new_session_is_a_plain_failure() {
        let err = parse_result(&output(1, "", NO_SESSION_STDERR), &fresh()).unwrap_err();
        assert!(matches!(err, AgentError::Failed(CLAUDE, _)), "{err:?}");
    }

    #[test]
    fn an_error_result_is_a_failure_carrying_its_text() {
        let text = RESULT.replace("\"is_error\":false", "\"is_error\":true");
        let err = parse_result(&output(1, &text, ""), &fresh()).unwrap_err();
        assert_eq!(err, AgentError::Failed(CLAUDE, "ok".into()));
    }

    #[test]
    fn a_result_without_its_cost_is_unreadable() {
        let text = RESULT.replace("\"total_cost_usd\":0.0176483,", "");
        let err = parse_result(&output(0, &text, ""), &fresh()).unwrap_err();
        assert!(matches!(err, AgentError::Unreadable(CLAUDE, _)), "{err:?}");
    }

    #[test]
    fn no_json_and_a_failed_exit_reports_stderr() {
        let err = parse_result(&output(1, "", "not logged in"), &fresh()).unwrap_err();
        assert_eq!(err, AgentError::Failed(CLAUDE, "not logged in".into()));
    }

    #[test]
    fn no_json_and_a_clean_exit_is_unreadable() {
        let err = parse_result(&output(0, "hello", ""), &fresh()).unwrap_err();
        assert_eq!(err, AgentError::Unreadable(CLAUDE, "hello".into()));
    }

    #[test]
    fn a_cost_is_kept_to_the_billionth_and_refuses_nonsense() {
        assert_eq!(Cost::from_usd(0.16096939999999998), Some(Cost(160_969_400)));
        assert_eq!(Cost::from_usd(0.0), Some(Cost(0)));
        assert_eq!(Cost::from_usd(-0.01), None);
        assert_eq!(Cost::from_usd(f64::NAN), None);
        assert_eq!(Cost(22_917_000).usd(), 0.022917);
    }

    #[derive(Debug, Default)]
    struct Recorded(std::sync::Mutex<Vec<(u32, String)>>);

    impl LambLabels for Recorded {
        fn label(&self, pid: u32, label: &str) {
            self.0.lock().unwrap().push((pid, label.to_owned()));
        }
    }

    #[test]
    fn a_call_s_lamb_carries_its_issue_and_role() {
        let dir = tempfile::tempdir().unwrap();
        let (claude, pid_file) = (dir.path().join("claude"), dir.path().join("pid"));
        std::fs::write(dir.path().join("result.json"), RESULT).unwrap();
        let script = format!(
            "#!/bin/sh\necho $$ > {}\ncat {}/result.json\n",
            pid_file.display(),
            dir.path().display()
        );
        crate::test::write_script(&claude, &script);
        let lambs = Arc::new(Recorded::default());
        let cli = ClaudeCli {
            program: claude.into(),
            ..ClaudeCli::labelling(Arc::clone(&lambs) as Arc<dyn LambLabels>)
        }
        .sandboxed(Arc::new(OpenSandbox::default()), dir.path().join("home"));
        for role in [Role::Worker, Role::Reviewer] {
            let mut call = call(role, fresh());
            call.cwd = dir.path().to_owned();
            call.settings = dir.path().join("settings.json");
            cli.prepare(&call).unwrap();
            cli.run(&call, &Ending::default()).unwrap();
        }
        let labels = lambs.0.lock().unwrap().clone();
        let names: Vec<&str> = labels.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(names, ["#6 worker", "#6 reviewer"]);
        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(labels[1].0, pid, "the label is not on the process that ran");
    }
}
