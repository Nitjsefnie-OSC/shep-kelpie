//! pi, headless as `pi -p`, one process per call, on a model an
//! OpenAI-compatible server runs
//!
//! Each call runs whole inside kelpie's sandbox, as Claude Code's do. pi has
//! no sandbox of its own and no permission prompts. Kelpie gives it a home of
//! its own beside the call's settings, holding the one model the call runs
//! and its sessions, so the maintainer's own pi setup never loads. A fenced
//! call also loads an extension that runs kelpie's checks before each
//! command and file write. pi starts no MCP servers. The sandbox allows no
//! model host: pi's calls go to a forwarder outside it, which passes the
//! server only the chat completions call. A model behind a gateway gets the
//! gateway's key from the forwarder, and each call on one holds the gateway's
//! lease on its model from start to end, where its kind offers one.

use std::ffi::OsString;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;

use self::adopt::{Carried, kept_server};
use self::compact::Tokens;
use super::claude::sandbox::call_policy;
use super::claude::{ClaudeCli, LambLabels};
use super::forwarder::Forwarder;
use super::process::record::CallFiles;
use super::process::{Processes, RunError};
use super::skills::skill_dirs;
use crate::fence;
use crate::forwarder::{Upstream, WORKER_HOST};
use crate::guard::{AFTER_FLAG, FOLDER_FLAG};
use crate::ports::{
    Adopted, AgentCall, AgentError, AgentReply, Agents, COMPACT, CallActivity, Ending, Fence,
    Forward, Kept, Policy, Role, Sandbox, Session, SessionId, Stand, Taking, Tools, Transcript,
    Unheld, Usage, Wait,
};
use crate::settings::{AgentHarness, Gateways, Harness, HookPoint, ModelHost, ModelServer};
use crate::skills::split_command;

mod activity;
mod adopt;
mod compact;

#[cfg(test)]
mod activity_tests;

#[cfg(test)]
mod adopt_tests;

#[cfg(test)]
mod compact_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod escapes;

#[cfg(test)]
mod guard_tests;

#[cfg(test)]
mod gateway_tests;

#[cfg(test)]
mod unreachable_tests;

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod window_tests;

/// The oldest pi kelpie is built for: built-in MCP needs 0.99 or later, and
/// compaction events carry a `reason` from 1.0.4
pub const PI_MINIMUM: &str = "1.0.4";

/// This adapter's harness, as its errors and the fence name it
const PI: Harness = Harness::Pi;

/// The provider kelpie's own `models.json` names, for the call's one model
const PROVIDER: &str = "kelpie";

/// The extension that runs kelpie's checks, with `__CHECKS__` to fill in
const GUARD: &str = include_str!("pi/guard.ts");

/// pi's tools for each kind, by its own names
///
/// A reviewer that may write has `write` alone, as Claude's has `Write` alone.
fn tool_names(tools: Tools, reads: bool, writes: bool) -> Option<&'static str> {
    match tools {
        Tools::Work => Some("read,bash,edit,write,grep,find,ls"),
        Tools::Review if writes => Some("read,grep,find,ls,write"),
        // The issue writer runs on Claude Code alone, whose hook is its guard.
        // pi has no web tool, so a researcher on it reads what kelpie wrote it and no more.
        Tools::Review | Tools::Issues | Tools::Retro | Tools::Advise | Tools::Research => {
            Some("read,grep,find,ls")
        }
        // The project manager runs on Claude Code alone, so pi only ever reads for it.
        Tools::Answer | Tools::Pm if reads => Some("read"),
        Tools::Answer | Tools::Pm => None,
    }
}

/// Headless pi, one `pi -p` process per call, each inside the sandbox
///
/// It shares Claude Code's calls in flight, so stopping either stops both.
#[derive(Debug, Clone)]
pub struct PiCli {
    processes: Processes,
    program: OsString,
    lambs: Option<Arc<dyn LambLabels>>,
    sandbox: Arc<dyn Sandbox>,
    // What a call with no fence may not read: the shepherd's home
    unfenced_no_read: Vec<String>,
    gateways: Gateways,
    // What each running call keeps in memory that a handover must carry
    carried: Carried,
}

impl ClaudeCli {
    /// pi, run the way this runs Claude Code: same sandbox, same lamb labels,
    /// and stopped with it
    pub fn pi(&self) -> PiCli {
        PiCli {
            processes: self.processes.clone(),
            program: "pi".into(),
            lambs: self.lambs.clone(),
            sandbox: Arc::clone(&self.sandbox),
            unfenced_no_read: self.unfenced_no_read.clone(),
            gateways: Gateways::default(),
            carried: Carried::default(),
        }
    }
}

impl PiCli {
    /// Sends a model behind a gateway through `gateways`
    pub fn with_gateways(self, gateways: Gateways) -> Self {
        Self { gateways, ..self }
    }

    /// Runs `program` in place of `pi`, as a stand-in script does
    #[cfg(test)]
    pub(crate) fn with_program(self, program: PathBuf) -> Self {
        Self {
            program: program.into(),
            ..self
        }
    }
}

impl Agents for PiCli {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        let server = server(call)?;
        let upstream = self.upstream(server)?;
        refuse_unsupported(call)?;
        let files = Files::of(call);
        let setup = |what: &Path, e: std::io::Error| {
            AgentError::Setup(format!("cannot write {}: {}", what.display(), e.kind()))
        };
        std::fs::create_dir_all(files.sessions()).map_err(|e| setup(&files.sessions(), e))?;
        let models = json!({ "providers": { PROVIDER: {
            "baseUrl": upstream.worker_url(),
            "api": "openai-completions",
            // pi lists no model without one. The forwarder sends a gateway's own key.
            "apiKey": PROVIDER,
            "models": [{
                "id": call.model,
                "contextWindow": server.context.get(),
                "reasoning": true,
            }],
        } } });
        let text = serde_json::to_string_pretty(&models).expect("models are JSON");
        let written = [
            (files.home.join("models.json"), text.as_str()),
            // Written here, pi only reads them, and locks them beside them.
            (files.home.join("auth.json"), "{}"),
            (files.home.join("models-store.json"), "{}"),
        ];
        for (path, text) in written {
            std::fs::write(&path, text).map_err(|e| setup(&path, e))?;
        }
        if let Some(fence) = &call.reach.fence {
            let guard = files.guard();
            std::fs::write(&guard, guard_extension(fence)).map_err(|e| setup(&guard, e))?;
        }
        // A compact call of any role loads it, the project manager's included.
        if loads_compact(call) {
            let compact = files.compact();
            let again = call.compact_window.map(|_| files.prompt());
            std::fs::write(&compact, compact::extension(again.as_deref()))
                .map_err(|e| setup(&compact, e))?;
        }
        if call.compact_window.is_some() {
            let prompt = files.prompt();
            std::fs::write(&prompt, compact::handed_back(call)).map_err(|e| setup(&prompt, e))?;
        }
        // The home outlives the call, so an earlier call's reserve goes with its window.
        let path = files.settings();
        match call
            .compact_window
            .and_then(|w| compact::settings(server.context.get(), w))
        {
            Some(settings) => {
                let text = serde_json::to_string_pretty(&settings).expect("settings are JSON");
                std::fs::write(&path, text).map_err(|e| setup(&path, e))?;
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(setup(&path, e)),
                _ => {}
            },
        }
        Ok(())
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        // A session that is not there never waits for a lease.
        refuse_missing_session(call)?;
        let label = call.label();
        let spawned = |pid| {
            if let Some(lambs) = &self.lambs {
                lambs.label(pid, &label);
            }
        };
        let output = self.leased(call, ending, |lease| {
            // The forwarder stays until the call has ended.
            let (mut command, forwarder) = self.sandboxed_command(call, lease)?;
            let _noted = self.carried.note(
                call,
                Kept {
                    forwarder: forwarder.socket.clone(),
                    server: kept_server(&server(call)?.host),
                    session: call.session.id().clone(),
                    lease: lease.map(str::to_owned),
                },
            );
            (self.processes)
                .output_of_call(&mut command, call, ending, &spawned)
                .map_err(|e| match e {
                    RunError::Io(e) => AgentError::Spawn(PI, e.to_string()),
                    RunError::Stopped => AgentError::Stopped,
                    RunError::TimedOut => AgentError::TimedOut(PI),
                })
        })??;
        read(call, &output)
    }

    fn adoption(&self, call: &AgentCall) -> Result<Adopted, String> {
        self.adoption_of(call)
    }

    fn adopt(&self, adopted: &Adopted, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.adopt_call(adopted, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        Some(Transcript {
            folder: Files::of(call).sessions(),
            threads: None,
        })
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        activity::of_stdout(&CallFiles::of(call).stdout)
    }
}

impl PiCli {
    /// What the forwarder passes the call's chat calls to, a gateway's key included
    fn upstream(&self, server: &ModelServer) -> Result<Upstream, AgentError> {
        self.gateways
            .upstream(&server.host)
            .map_err(AgentError::Setup)
    }

    // A call on a gateway's model holds a lease on it, so the gateway
    // evicts nothing it runs on midway and queues the call behind any other
    // lease. Its ceiling counts from the grant. A lease the gateway took back
    // ended the call, whatever the run says of it. The run is told the
    // lease's id, so its chat calls name the turn they belong to.
    fn leased<T>(
        &self,
        call: &AgentCall,
        ending: &Ending,
        run: impl FnOnce(Option<&str>) -> T,
    ) -> Result<T, AgentError> {
        let server = server(call)?;
        let ModelHost::Gateway(name) = &server.host else {
            return Ok(run(None));
        };
        let upstream = self.upstream(server)?;
        let port = self.gateways.port_named(name).map_err(AgentError::Setup)?;
        let note = format!("kelpie {}", call.label());
        let tell = |stand| match stand {
            Stand::Asking | Stand::Turn => ending.wait(Wait::Lease),
            Stand::Busy => ending.wait(Wait::Busy),
            Stand::Granted => ending.begin(),
        };
        let taking = Taking {
            model: &call.model,
            note: &note,
            give_up: &|| ending.asked() || self.processes.stopping(),
            tell: &tell,
            ending,
        };
        port.around_leased(&upstream, &taking, run)
            .map_err(|unheld| match unheld {
                Unheld::GaveUp if self.processes.stopping() => AgentError::Stopped,
                Unheld::GaveUp => AgentError::TimedOut(PI),
                Unheld::Refused(why) => AgentError::Setup(format!(
                    "cannot take a lease on {} from gateway {name}: {why}",
                    call.model
                )),
                Unheld::Lost(why) => AgentError::Failed(PI, why),
            })
    }

    // The forwarder names `lease`, the gateway lease the call holds, on a worker's
    // chat calls. A reviewer's session holds one too but names none.
    fn sandboxed_command(
        &self,
        call: &AgentCall,
        lease: Option<&str>,
    ) -> Result<(Command, Forwarder), AgentError> {
        let upstream = self.upstream(server(call)?)?;
        let upstream = match lease.filter(|_| call.role == Role::Worker) {
            Some(id) => upstream.with_lease(id),
            None => upstream,
        };
        let files = Files::of(call);
        match std::fs::remove_dir_all(&files.scratch) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(AgentError::Setup(format!(
                    "cannot empty {}: {}",
                    files.scratch.display(),
                    e.kind()
                )));
            }
            _ => {}
        }
        std::fs::create_dir_all(&files.scratch).map_err(|e| {
            AgentError::Setup(format!(
                "cannot make {}: {}",
                files.scratch.display(),
                e.kind()
            ))
        })?;
        let folder = call.settings.parent().unwrap_or(Path::new("/"));
        let forwarder = Forwarder::open(folder, upstream.clone()).map_err(AgentError::Setup)?;
        // `env` runs inside the sandbox, after the sandbox has set its own variables.
        let mut command = crate::spawn::command("env");
        command
            .args(env_args(call, &files))
            .arg(&self.program)
            .args(argv(call, &files))
            .current_dir(&call.cwd);
        let policy = policy(
            call,
            &files,
            &upstream,
            &forwarder.socket,
            &self.unfenced_no_read,
        );
        let wrapped = self
            .sandbox
            .wrap(&policy, &files.sandbox_settings, &command)
            .map_err(|e| AgentError::Setup(e.to_string()))?;
        Ok((wrapped, forwarder))
    }
}

// A resumed session pi never wrote is a call that cannot run.
fn refuse_missing_session(call: &AgentCall) -> Result<(), AgentError> {
    match &call.session {
        Session::Resume(id) if session_file(&Files::of(call).sessions(), id).is_none() => {
            Err(AgentError::NoSession(PI, id.clone()))
        }
        _ => Ok(()),
    }
}

/// The server the call's model is on, which every pi call needs
fn server(call: &AgentCall) -> Result<&ModelServer, AgentError> {
    match &call.harness {
        AgentHarness::Pi(server) => Ok(server),
        _ => Err(AgentError::Setup(
            "kelpie routed a call to pi that names no model server".into(),
        )),
    }
}

// What a worker on Claude Code has and pi cannot give it.
fn refuse_unsupported(call: &AgentCall) -> Result<(), AgentError> {
    if call
        .reach
        .fence
        .as_ref()
        .is_some_and(|f| !f.hooks.is_empty())
    {
        return Err(AgentError::Setup(
            "pi cannot run `worker.guard_hooks`, which are Claude Code hooks: \
             remove them, move the check to a `command` or `write` hook in `hooks`, which \
             runs on every harness, or put the worker on Claude Code"
                .into(),
        ));
    }
    Ok(())
}

// A compact call, and a call bounded by a window that hands its prompt back
// after pi compacts it, load the compact extension.
fn loads_compact(call: &AgentCall) -> bool {
    call.prompt == COMPACT || call.compact_window.is_some()
}

/// The files one call keeps beside its settings path
struct Files {
    /// pi's own home for the call: the model, and the sessions folder
    home: PathBuf,
    /// The call's temporary files, emptied before each call
    scratch: PathBuf,
    /// Where the sandbox's settings go
    sandbox_settings: PathBuf,
    /// The guard extension, outside anything the call may write
    guard_path: PathBuf,
    /// The extension a compact call loads, outside anything the call may write
    compact_path: PathBuf,
    /// The turn's prompt, which the compact extension hands back after a compaction
    prompt_path: PathBuf,
}

impl Files {
    fn of(call: &AgentCall) -> Self {
        Self {
            home: call.settings.with_extension("pi"),
            scratch: call.settings.with_extension("tmp"),
            sandbox_settings: call.settings.with_extension("sandbox.json"),
            guard_path: call.settings.with_extension("guard.ts"),
            compact_path: call.settings.with_extension("compact.ts"),
            prompt_path: super::claude::settings::prompt_file(&call.settings),
        }
    }

    fn sessions(&self) -> PathBuf {
        self.home.join("sessions")
    }

    fn guard(&self) -> PathBuf {
        self.guard_path.clone()
    }

    fn compact(&self) -> PathBuf {
        self.compact_path.clone()
    }

    fn prompt(&self) -> PathBuf {
        self.prompt_path.clone()
    }

    // pi's own settings, which it reads from its home.
    fn settings(&self) -> PathBuf {
        self.home.join("settings.json")
    }

    // pi takes a lock folder beside each store it reads at start, even when
    // it changes nothing there.
    fn locks(&self) -> [PathBuf; 3] {
        [
            "auth.json.lock",
            "models-store.json.lock",
            "settings.json.lock",
        ]
        .map(|lock| self.home.join(lock))
    }
}

/// The session file pi keeps for `id`, named `<time>_<id>.jsonl`
fn session_file(sessions: &Path, id: &SessionId) -> Option<PathBuf> {
    let suffix = format!("_{}.jsonl", id.0);
    std::fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&suffix))
        })
}

fn env_args(call: &AgentCall, files: &Files) -> Vec<OsString> {
    let mut vars: Vec<(String, OsString)> = vec![
        // pi is Node, whose fetch ignores the sandbox's proxy without it.
        ("NODE_USE_ENV_PROXY".into(), "1".into()),
        ("PI_CODING_AGENT_DIR".into(), files.home.clone().into()),
        ("TMPDIR".into(), files.scratch.clone().into()),
        // zsh ignores `TMPDIR` for a heredoc and writes under this prefix.
        ("TMPPREFIX".into(), files.scratch.join("zsh").into()),
    ];
    if let Some(fence) = &call.reach.fence {
        vars.extend(
            fence
                .env
                .iter()
                .map(|(name, value)| (name.clone(), value.clone().into())),
        );
    }
    vars.into_iter()
        .map(|(name, value)| {
            let mut pair = OsString::from(format!("{name}="));
            pair.push(value);
            pair
        })
        .collect()
}

// pi loads nothing of the maintainer's or the repo's own: no extensions,
// skills, prompt templates or themes it finds, and no project-local files.
// The skills it loads are the ones named with `--skill`, which need no project trust.
// It does read the repo's AGENTS.md and CLAUDE.md, as Claude Code does.
fn argv(call: &AgentCall, files: &Files) -> Vec<OsString> {
    let compacting = call.prompt == COMPACT;
    let (prompt, skill) = match compacting {
        true => (compact::COMMAND.to_owned(), None),
        false => skill_prompt(call),
    };
    let mut argv: Vec<OsString> = vec![
        "-p".into(),
        "--mode".into(),
        "json".into(),
        "--model".into(),
        format!("{PROVIDER}/{}", call.model).into(),
        "--thinking".into(),
        call.effort.as_str().into(),
        "--no-extensions".into(),
        "--no-skills".into(),
        "--no-prompt-templates".into(),
        "--no-themes".into(),
        "--no-approve".into(),
        "--offline".into(),
        "--session-dir".into(),
        files.sessions().into(),
        "--session-id".into(),
        call.session.id().0.as_str().into(),
    ];
    let writes = call
        .reach
        .fence
        .as_ref()
        .is_some_and(|f| !f.write.is_empty());
    let tools = tool_names(call.tools, !call.reach.read.is_empty(), writes);
    match tools {
        Some(names) => argv.extend(["--tools".into(), names.into()]),
        None => argv.push("--no-tools".into()),
    }
    // pi builds its system prompt afresh on every call, a resumed one too.
    if let Some(instructions) = &call.instructions {
        argv.extend(["--append-system-prompt".into(), instructions.into()]);
    }
    if call.reach.fence.is_some() {
        argv.extend(["--extension".into(), files.guard().into()]);
    }
    if loads_compact(call) {
        argv.extend(["--extension".into(), files.compact().into()]);
    }
    // The step's skill comes first, so its name wins a collision. Its text is in
    // the prompt, but any other skill needs `read` to open and only costs context.
    if let Some(skill) = skill {
        argv.extend(["--skill".into(), skill.into()]);
    }
    if tools.is_some() && !compacting {
        for dir in skill_dirs(call) {
            argv.extend(["--skill".into(), dir.into()]);
        }
    }
    argv.extend(["--".into(), prompt.into()]);
    argv
}

/// The prompt as pi takes it, and the skill folder its slash command runs
///
/// Kelpie starts a step's prompt with Claude Code's `/<plugin>:<skill>`.
/// pi runs a skill it is given as `/skill:<name>`. A skill none of the
/// call's plugins holds leaves the prompt as it is.
fn skill_prompt(call: &AgentCall) -> (String, Option<PathBuf>) {
    match step_skill(call) {
        Some(step) => (
            format!("/skill:{} {}", step.name, step.rest),
            Some(step.dir),
        ),
        None => (call.prompt.clone(), None),
    }
}

/// The skill a step's prompt starts with, as Claude Code's `/<plugin>:<skill>`
pub(super) struct StepSkill {
    /// The skill's name
    pub(super) name: String,
    /// Its folder, which holds its `SKILL.md`
    pub(super) dir: PathBuf,
    /// The prompt after the command
    pub(super) rest: String,
}

/// The skill `call`'s prompt runs, from one of its plugins, if any
pub(super) fn step_skill(call: &AgentCall) -> Option<StepSkill> {
    let (Some(command), rest) = split_command(&call.prompt) else {
        return None;
    };
    let (plugin, skill) = command.trim_start_matches('/').split_once(':')?;
    let dir = call
        .plugin_dirs
        .iter()
        .filter(|dir| plugin_name(dir).as_deref() == Some(plugin))
        .map(|dir| dir.join("skills").join(skill))
        .find(|dir| dir.join("SKILL.md").is_file())?;
    Some(StepSkill {
        name: skill.to_owned(),
        dir,
        rest: rest.to_owned(),
    })
}

// A plugin's name, from its manifest
fn plugin_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest["name"].as_str().map(str::to_owned)
}

/// The guard extension's source for a call fenced by `fence`
fn guard_extension(fence: &Fence) -> String {
    let guard = &fence.guard;
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let mut commands = vec![
        "guard".to_owned(),
        text(&guard.git_common_dir),
        text(&guard.worktree),
    ];
    commands.extend(
        guard
            .folders
            .iter()
            .map(|p| format!("{FOLDER_FLAG}{}", p.display())),
    );
    commands.extend(guard.hooks.flag());
    // The project's `write` hooks ride on `confine`, and its `after_command` ones on
    // the guard again, run on each command's result.
    let mut confine = vec![
        "confine".to_owned(),
        text(&guard.worktree),
        text(&guard.build),
    ];
    if let Some(flag) = guard
        .hooks
        .flag()
        .filter(|_| guard.hooks.has(HookPoint::Write))
    {
        confine.push(flag);
    }
    let after = guard.hooks.has(HookPoint::AfterCommand).then(|| {
        let mut after = commands.clone();
        after.push(AFTER_FLAG.to_owned());
        after
    });
    let checks = json!({
        "kelpie": text(&guard.kelpie),
        "confine": confine,
        "guard": commands,
        "after": after,
    });
    GUARD.replace("__CHECKS__", &checks.to_string())
}

/// The whole call's policy: its fence, or none, and what pi itself needs
///
/// The model's host is not in it. The one host pi may dial is the forwarder's.
fn policy(
    call: &AgentCall,
    files: &Files,
    upstream: &Upstream,
    forwarder: &Path,
    unfenced: &[String],
) -> Policy {
    let mut policy = call_policy(call, unfenced);
    policy.no_keychain = call.reach.block_keychain;
    // pi logs in nowhere, so no harness's login is its own, the maintainer's pi included.
    let logins = fence::HARNESSES.iter().flat_map(|own| own.credentials);
    policy.no_read.extend(logins.map(|&path| path.to_owned()));
    policy.write.extend(
        [files.sessions(), files.scratch.clone()]
            .into_iter()
            .chain(files.locks()),
    );
    // Node aborts at start when it cannot stat its own stdout and stderr,
    // the files kelpie opens for them.
    let outputs = CallFiles::of(call);
    policy.read.extend(
        [
            files.home.clone(),
            files.guard(),
            files.compact(),
            files.scratch.clone(),
        ]
        .into_iter()
        .chain(call.compact_window.map(|_| files.prompt()))
        .chain([outputs.stdout, outputs.stderr])
        .chain(call.instructions.iter().cloned())
        .chain(call.plugin_dirs.iter().cloned())
        .chain(call.reach.read.iter().cloned()),
    );
    deny_the_server(&mut policy, upstream);
    policy.forward = Some(Forward {
        host: WORKER_HOST.to_owned(),
        socket: forwarder.to_owned(),
    });
    policy
}

// Whatever `allowed_domains` says, the sandbox never reaches the model server
// or this machine's loopback by name or by the address the server resolves to.
fn deny_the_server(policy: &mut Policy, upstream: &Upstream) {
    let bracketed = |host: &str| match host.contains(':') {
        true => format!("[{host}]"),
        false => host.to_owned(),
    };
    for host in [upstream.host(), "localhost", "127.0.0.1", "::1"] {
        let host = bracketed(host);
        if !policy.denied_hosts.contains(&host) {
            policy.denied_hosts.push(host);
        }
    }
    let resolved = upstream.address().to_socket_addrs().into_iter().flatten();
    for address in resolved {
        let address = match address.ip() {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address.ip(), IpAddr::V4),
            v4 => v4,
        };
        let address = address.to_string();
        if !policy.denied_addresses.contains(&address) {
            policy.denied_addresses.push(address);
        }
    }
}

/// How much of pi's output an error carries: its end, where the reason is
const ERROR_TAIL: usize = 2048;

// Whether pi's error says its model host could not be reached: a 502 or 503
// as the SDK leads its message with the status, or a connection refused or
// reset. Any other error is the call's own, and so is one that leads with
// another status, whatever its text goes on to mention.
fn reaches_no_host(detail: &str) -> bool {
    let lower = detail.trim_start().to_ascii_lowercase();
    let digits = lower.bytes().take_while(u8::is_ascii_digit).count();
    let leads_with_status =
        digits == 3 && !lower[digits..].starts_with(|c: char| c.is_ascii_alphanumeric());
    if leads_with_status {
        return matches!(&lower[..digits], "502" | "503");
    }
    [
        "econnrefused",
        "econnreset",
        "connection refused",
        "connection reset",
    ]
    .iter()
    .any(|sign| lower.contains(sign))
}

// The last `ERROR_TAIL` bytes of `text`, since a worker's output runs to
// megabytes and an error reaches a ruling, the state file and the webhook.
fn tail(text: &str) -> &str {
    let mut start = text.len().saturating_sub(ERROR_TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

// A compact call's output is read for its compaction, any other call's for its turn.
fn read(call: &AgentCall, output: &Output) -> Result<AgentReply, AgentError> {
    read_as(call.prompt == COMPACT, output, call.session.id())
}

// The same for a call another image carried, which knows only whether it compacts.
fn read_as(
    compacting: bool,
    output: &Output,
    session: &SessionId,
) -> Result<AgentReply, AgentError> {
    match compacting {
        true => compact::parse(output, session),
        false => parse_result(output, session),
    }
}

// An error for a call that failed with `detail`, unreachable where it says the
// model host could not be reached.
fn failed(detail: &str) -> AgentError {
    if reaches_no_host(detail) {
        AgentError::Unreachable(PI, detail.to_owned())
    } else {
        AgentError::Failed(PI, detail.to_owned())
    }
}

// `pi -p --mode json` prints one event per line: a session header, then
// every message as it ends. The call's usage is the sum of its assistant
// messages and its own compactions, its context the last request's prompt,
// and its answer the last message's text. A local model has no price.
fn parse_result(output: &Output, asked: &SessionId) -> Result<AgentReply, AgentError> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        message: Option<Message>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Message {
        role: String,
        #[serde(default)]
        content: Vec<Content>,
        #[serde(default)]
        usage: Option<Tokens>,
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        error_message: Option<String>,
    }
    #[derive(Deserialize)]
    struct Content {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        text: Option<String>,
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut session = None;
    let mut usage = Usage::default();
    let mut context = None;
    let mut last: Option<Message> = None;
    // Whether the latest message to end could not be read, so `last` is stale.
    let mut unread = false;
    for text in stdout.lines().filter(|l| l.starts_with('{')) {
        let Ok(line) = serde_json::from_str::<Line>(text) else {
            unread |= text.contains(r#""type":"message_end""#);
            continue;
        };
        if line.kind == "message_end" {
            unread = false;
        }
        match (line.kind.as_str(), line.message) {
            ("session", _) => session = line.id,
            ("message_end", Some(message)) if message.role == "assistant" => {
                if let Some(u) = &message.usage {
                    usage += u.usage();
                }
                // The latest message alone: an earlier one's prompt is stale.
                context = message.usage.as_ref().map(Tokens::context);
                last = Some(message);
            }
            // pi's own threshold compaction, which runs inside a turn.
            ("compaction_end", _) => usage += compact::usage_of(text),
            _ => {}
        }
    }
    if !output.status.success() {
        let why = last.as_ref().and_then(|m| m.error_message.clone());
        return Err(failed(why.as_deref().unwrap_or(tail(&stderr))));
    }
    let (Some(session), Some(last), false) = (session, last, unread) else {
        return Err(AgentError::Unreadable(PI, tail(&stdout).to_owned()));
    };
    match last.stop_reason.as_deref() {
        Some("stop") => {}
        Some("length") => return Err(failed("the model ran out of output tokens")),
        Some("error" | "aborted") => {
            let why = last.error_message.as_deref();
            return Err(failed(why.unwrap_or("no reason given")));
        }
        other => {
            let why = other.unwrap_or("none");
            return Err(failed(&format!("the model stopped for the reason `{why}`")));
        }
    }
    if session != asked.0 {
        return Err(AgentError::Unreadable(
            PI,
            format!("pi answered in session {session}, not {}", asked.0),
        ));
    }
    let text = last
        .content
        .iter()
        .filter(|c| c.kind == "text")
        .filter_map(|c| c.text.as_deref())
        .collect();
    Ok(AgentReply {
        session_id: SessionId(session),
        text,
        usage,
        session_cost: None,
        context,
    })
}
