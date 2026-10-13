//! goose, headless as `goose run`, one process per call, on a model an
//! OpenAI-compatible server runs
//!
//! Each call runs whole inside kelpie's sandbox, as pi's do. goose has no
//! sandbox of its own here. Kelpie gives it a root of its own beside the
//! call's settings, so the maintainer's own goose setup never loads, and loads
//! it no extension: the model is offered no tool, so it reads what kelpie
//! wrote it and answers. Only the researcher runs on goose for now. The sandbox
//! allows no model host: goose's calls go to a forwarder outside it, which
//! passes the server only the chat completions call. A model behind a gateway
//! gets the gateway's key from the forwarder, and each call on one holds the
//! gateway's lease on its model from start to end, where its kind offers one.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use self::files::Files;
use self::output::Spent;
use super::claude::sandbox::call_policy;
use super::claude::{ClaudeCli, LambLabels};
use super::forwarder::Forwarder;
use super::pi::deny_the_server;
use super::process::record::CallFiles;
use super::process::{Processes, RunError};
use crate::fence;
use crate::forwarder::{Upstream, WORKER_HOST};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, Ending, Forward, Policy, Role, Sandbox, Session,
    Stand, Taking, Tools, Unheld, Wait,
};
use crate::settings::{AgentHarness, Gateways, Harness, ModelHost, ModelServer};

mod files;
mod output;

#[cfg(test)]
mod gateway_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unreachable_tests;

/// The oldest goose kelpie is built for: `on_failure: block` hooks need 1.48 or
/// later, and 1.53.0 is the source the adapter is written from. The target is 1.54.0.
pub const GOOSE_MINIMUM: &str = "1.53.0";

/// This adapter's harness, as its errors name it
const GOOSE: Harness = Harness::Goose;

/// The key goose is given for its server. The forwarder sends a gateway's own.
const PLACEHOLDER_KEY: &str = "kelpie";

/// The project folders goose loads plugins and their settings from, whatever
/// `--no-profile` says
const PROJECT_FILES: [&str; 2] = [".agents/plugins", ".config/goose"];

/// Headless goose, one `goose run` process per call, each inside the sandbox
///
/// It shares Claude Code's calls in flight, so stopping either stops both.
#[derive(Debug, Clone)]
pub struct GooseCli {
    processes: Processes,
    program: OsString,
    lambs: Option<Arc<dyn LambLabels>>,
    sandbox: Arc<dyn Sandbox>,
    // What a call with no fence may not read: the shepherd's home
    unfenced_no_read: Vec<String>,
    gateways: Gateways,
}

impl ClaudeCli {
    /// goose, run the way this runs Claude Code: same sandbox, same lamb
    /// labels, and stopped with it
    pub fn goose(&self) -> GooseCli {
        GooseCli {
            processes: self.processes.clone(),
            program: "goose".into(),
            lambs: self.lambs.clone(),
            sandbox: Arc::clone(&self.sandbox),
            unfenced_no_read: self.unfenced_no_read.clone(),
            gateways: Gateways::default(),
        }
    }
}

impl GooseCli {
    /// Sends a model behind a gateway through `gateways`
    pub fn with_gateways(self, gateways: Gateways) -> Self {
        Self { gateways, ..self }
    }

    /// Runs `program` in place of `goose`, as a stand-in script does
    #[cfg(test)]
    pub(crate) fn with_program(self, program: std::path::PathBuf) -> Self {
        Self {
            program: program.into(),
            ..self
        }
    }
}

impl Agents for GooseCli {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        let server = server(call)?;
        refuse_unsupported(call)?;
        self.upstream(server)?;
        system_text(call)?;
        Files::of(&call.settings).prepare()
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let files = Files::of(&call.settings);
        let id = call.session.id();
        // A session kelpie never started never waits for a lease.
        let before = match &call.session {
            Session::New(_) => {
                files.keep(id, Spent::default())?;
                Spent::default()
            }
            Session::Resume(_) => files
                .spent(id)?
                .ok_or_else(|| AgentError::NoSession(GOOSE, id.clone()))?,
        };
        let label = call.label();
        let spawned = |pid| {
            if let Some(lambs) = &self.lambs {
                lambs.label(pid, &label);
            }
        };
        let output = self.leased(call, ending, |lease| {
            // The forwarder stays until the call has ended.
            let (mut command, _forwarder) = self.sandboxed_command(call, lease)?;
            (self.processes)
                .output_of_call(&mut command, call, ending, &spawned)
                .map_err(|e| match e {
                    RunError::Io(e) => AgentError::Spawn(GOOSE, e.to_string()),
                    RunError::Stopped => AgentError::Stopped,
                    RunError::TimedOut => AgentError::TimedOut(GOOSE),
                })
        })??;
        let read = output::read(&output, &call.session, before);
        // The session's totals grow whatever the call's reply was.
        if let Some(spent) = read.spent {
            files.keep(id, spent)?;
        }
        read.reply
    }
}

impl GooseCli {
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
                Unheld::GaveUp => AgentError::TimedOut(GOOSE),
                Unheld::Refused(why) => AgentError::Setup(format!(
                    "cannot take a lease on {} from gateway {name}: {why}",
                    call.model
                )),
                Unheld::Lost(why) => AgentError::Failed(GOOSE, why),
            })
    }

    // The forwarder names `lease`, the gateway lease the call holds, on a worker's
    // chat calls. Any other role's call holds one too but names none.
    fn sandboxed_command(
        &self,
        call: &AgentCall,
        lease: Option<&str>,
    ) -> Result<(Command, Forwarder), AgentError> {
        let server = server(call)?;
        let upstream = self.upstream(server)?;
        let upstream = match lease.filter(|_| call.role == Role::Worker) {
            Some(id) => upstream.with_lease(id),
            None => upstream,
        };
        let files = Files::of(&call.settings);
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
            .args(env_args(call, server, &files, &upstream))
            .arg(&self.program)
            .args(argv(call)?)
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

/// The server the call's model is on, which every goose call needs
fn server(call: &AgentCall) -> Result<&ModelServer, AgentError> {
    match &call.harness {
        AgentHarness::Goose(server) => Ok(server),
        _ => Err(AgentError::Setup(
            "kelpie routed a call to goose that names no model server".into(),
        )),
    }
}

// What goose cannot give a call yet. The agent files already refuse a role
// goose does not take, so this is the second line.
fn refuse_unsupported(call: &AgentCall) -> Result<(), AgentError> {
    let opens = |who: &str, slice: &str| {
        Some(format!(
            "goose cannot run {who} yet: {slice} opens it, and only a researcher runs on it until then"
        ))
    };
    let refused = match (call.role, call.tools) {
        (Role::Researcher, _) => None,
        (Role::Worker, Tools::Retro) => opens("a retro", "#789"),
        (Role::Worker, _) => opens("a worker", "#788"),
        (Role::Reviewer, _) => opens("a reviewer", "#789"),
        (Role::Advisor, _) => opens("an advisor", "#789"),
        (Role::IssueWriter | Role::Pm, _) => Some(format!(
            "goose cannot run the {}: it runs on Claude Code alone",
            call.role.as_str()
        )),
    };
    if let Some(why) = refused {
        return Err(AgentError::Setup(why));
    }
    if (call.reach.fence.as_ref()).is_some_and(|f| !f.hooks.is_empty()) {
        return Err(AgentError::Setup(
            "goose cannot run `worker.guard_hooks`, which are Claude Code hooks: \
             remove them, move the check to a `command` or `write` hook in `hooks`, which \
             runs on every harness, or put the worker on Claude Code"
                .into(),
        ));
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

// goose takes its system prompt as text, not as a path.
fn system_text(call: &AgentCall) -> Result<Option<String>, AgentError> {
    let Some(path) = &call.instructions else {
        return Ok(None);
    };
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|e| AgentError::Setup(format!("cannot read {}: {}", path.display(), e.kind())))
}

// The variables that move goose into the call's own root and point it at the
// forwarder. `GOOSE_MODE` is `chat`: the call loads no extension, so there is
// no tool for `auto` to allow, and goose rejects one that appears anyway.
fn env_args(
    call: &AgentCall,
    server: &ModelServer,
    files: &Files,
    upstream: &Upstream,
) -> Vec<OsString> {
    let mut vars: Vec<(String, OsString)> = vec![
        ("GOOSE_PATH_ROOT".into(), files.root.clone().into()),
        ("GOOSE_PROVIDER".into(), "openai".into()),
        ("GOOSE_MODEL".into(), call.model.as_str().into()),
        (
            "GOOSE_CONTEXT_LIMIT".into(),
            server.context.get().to_string().into(),
        ),
        ("GOOSE_MODE".into(), "chat".into()),
        ("OPENAI_BASE_URL".into(), upstream.worker_url().into()),
        ("OPENAI_API_KEY".into(), PLACEHOLDER_KEY.into()),
        ("GOOSE_DISABLE_KEYRING".into(), "1".into()),
        ("GOOSE_TELEMETRY_OFF".into(), "1".into()),
        ("TMPDIR".into(), files.scratch.clone().into()),
    ];
    if let Some(fence) = &call.reach.fence {
        vars.extend((fence.env.iter()).map(|(name, value)| (name.clone(), value.clone().into())));
    }
    vars.into_iter()
        .map(|(name, value)| {
            let mut pair = OsString::from(format!("{name}="));
            pair.push(value);
            pair
        })
        .collect()
}

// The prompt is one `--text=` argument, so one that starts with `-` is not read
// as a flag. goose is given no extension, and so no tool.
fn argv(call: &AgentCall) -> Result<Vec<OsString>, AgentError> {
    let mut argv: Vec<OsString> = vec![
        "run".into(),
        "--no-profile".into(),
        "--output-format".into(),
        "stream-json".into(),
        format!("--text={}", call.prompt).into(),
    ];
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
/// The model's host is not in it. The one host goose may dial is the forwarder's.
fn policy(
    call: &AgentCall,
    files: &Files,
    upstream: &Upstream,
    forwarder: &Path,
    unfenced: &[String],
) -> Policy {
    let mut policy = call_policy(call, unfenced);
    policy.no_keychain = call.reach.block_keychain;
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
