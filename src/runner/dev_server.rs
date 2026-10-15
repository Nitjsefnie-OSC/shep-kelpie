//! The project's dev server, started for each worker turn that has the browser
//!
//! A worker's sandbox binds no port and a headless turn has no background
//! tasks, so kelpie starts the dev server for it. The configuration is the
//! project's `.claude/launch.json` on `main`, and the server runs in a sandbox
//! of its own. It is planned as the turn is prepared, since the worker is told
//! its address, started on the turn's call thread, and stopped when that
//! thread's call returns, whatever ended it. It is a child of the runner, so
//! shep's stop reaches it (ADR 0005).

mod bump;
mod launch;

pub(super) use bump::{Browsed, BrowserProof};

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use super::Runner;
use super::claim::Notes;
use super::code_graph::who;
use super::instructions::DevServer;
use crate::metrics::{Feature, Recorder};
use crate::ports::{
    AgentCall, DevPort, DevRun, DevServers, Policy, READY_WITHIN, Reach, Session, Started,
};
use crate::settings::NonBlank;
use crate::work_item::WorkItem;

/// How many times a port is picked before one that no other plan holds is given up on
const PICKS: usize = 16;

/// The Mach service a dev server's file watcher looks up on macOS
const FS_EVENTS: &str = "com.apple.FSEvents";

/// Whether `main` of `checkout` has a launch file, which is when the dev server is on
pub(super) fn has_launch_file(checkout: &Path) -> bool {
    launch::exists(checkout)
}

/// The dev servers planned for turns not yet over, by the issue of their work item
#[derive(Debug, Default)]
pub(super) struct Planned {
    turns: Mutex<BTreeMap<u64, DevRun>>,
    // The ports of the servers a bump's proof runs, by the bump's pull request
    proofs: Mutex<BTreeMap<u64, u16>>,
}

impl Planned {
    // Replaces the plan held for `issue`, or drops it.
    pub(super) fn hold(&self, issue: u64, plan: Option<DevRun>) {
        let mut held = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        match plan {
            Some(plan) => held.insert(issue, plan),
            None => held.remove(&issue),
        };
    }

    // The ports the plans held name, which no other plan may take though nothing listens yet.
    fn claimed(&self) -> Vec<u16> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let proofs = self.proofs.lock().unwrap_or_else(PoisonError::into_inner);
        (turns.values().map(|plan| plan.port))
            .chain(proofs.values().copied())
            .collect()
    }

    pub(super) fn get(&self, issue: u64) -> Option<DevRun> {
        let held = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        held.get(&issue).cloned()
    }

    // Holds `port` for the proof of bump `pull_request`, or lets it go.
    pub(super) fn hold_proof(&self, pull_request: u64, port: Option<u16>) {
        let mut held = self.proofs.lock().unwrap_or_else(PoisonError::into_inner);
        match port {
            Some(port) => held.insert(pull_request, port),
            None => held.remove(&pull_request),
        };
    }
}

impl Runner {
    /// What a worker turn on `item` is told of the project's dev server, and
    /// the plan to start it, if the turn has the browser
    ///
    /// `None` when the turn has no browser, or when `main` has no launch file.
    /// A server that cannot be had is told too, and the turn runs without it.
    pub(super) fn dev_server(
        &self,
        item: &WorkItem,
        reach: &Reach,
        hosts: Option<&[NonBlank]>,
    ) -> Option<DevServer> {
        self.planned.hold(item.issue, None);
        let hosts = hosts?;
        let fence = reach.fence.as_deref()?;
        let without = |why: String| {
            self.metrics.feature_ran(Feature::DevServer, "failed");
            self.notes.push(format!(
                "issue #{}'s worker runs without a dev server: {why}",
                item.issue
            ));
            Some(DevServer::Without(why))
        };
        let skipped = || {
            self.metrics.feature_ran(Feature::DevServer, "skipped");
            None
        };
        let text = match launch::read(&self.settings.git.checkout) {
            Ok(Some(text)) => text,
            Ok(None) => return skipped(),
            Err(why) => return without(why),
        };
        let named = self.settings.browser.configuration.as_ref();
        let configuration = match launch::select(&text, named.map(NonBlank::as_str)) {
            Ok(Some(configuration)) => configuration,
            Ok(None) => return skipped(),
            Err(why) => return without(why),
        };
        let cwd = match launch::folder(&item.worktree, &configuration) {
            Ok(cwd) => cwd,
            Err(why) => return without(why),
        };
        let (port, picked) = match self.port_for(&configuration) {
            Ok(port) => port,
            Err(why) => return without(why),
        };
        let (args, env) = command_of(&configuration, port, picked);
        let log = item.build.join("dev-server.log");
        let plan = DevRun {
            program: configuration.runtime_executable,
            args,
            cwd,
            port,
            env,
            policy: Policy {
                write: vec![item.worktree.clone(), item.build.clone()],
                no_write: fence.no_write.clone(),
                no_read: fence.no_read.clone(),
                read: fence.read.clone(),
                hosts: hosts.iter().map(|h| h.as_str().to_owned()).collect(),
                verify_tls: true,
                no_keychain: self.app_covers(),
                listen: true,
                services: vec![FS_EVENTS.to_owned()],
                ..Policy::default()
            },
            settings: (self.paths.worker).join(format!("dev-server-{}.srt.json", item.issue)),
            log: log.clone(),
            ready_within: READY_WITHIN,
            gate: DevPort::default(),
        };
        self.planned.hold(item.issue, Some(plan));
        Some(DevServer::At { port, log })
    }

    // The port the server answers on, and whether it was picked: the
    // configured one when nothing holds it, else a free one under `autoPort`.
    fn port_for(&self, configuration: &launch::Configuration) -> Result<(u16, bool), String> {
        let servers = &self.ports.dev_servers;
        // Another work item's server may not have bound its port yet.
        let claimed = self.planned.claimed();
        let free = |port: u16| !claimed.contains(&port) && servers.free(port);
        let pick = || {
            for _ in 0..PICKS {
                match servers.pick() {
                    Ok(port) if claimed.contains(&port) => {}
                    other => return other,
                }
            }
            Err("every port it found was planned for another work item".to_owned())
        };
        match (configuration.port, configuration.auto_port) {
            (Some(port), _) if port != 0 && free(port) => Ok((port, false)),
            (Some(0), false) => {
                Err("the launch configuration's `port` is 0, which is no port".to_owned())
            }
            (Some(port), false) => Err(format!(
                "port {port} is taken, and the launch configuration has no `autoPort`"
            )),
            (_, true) => pick()
                .map(|port| (port, true))
                .map_err(|e| format!("no free port for it: {e}")),
            (None, false) => Err("the launch configuration names no `port`".to_owned()),
        }
    }

    /// The gate on the port of the dev server planned for `issue`'s turn
    ///
    /// It is shut until the server answers, so the browser opens no port that
    /// kelpie has not seen its own server answer on.
    pub(super) fn dev_gate(&self, issue: u64) -> DevPort {
        self.planned
            .get(issue)
            .map(|plan| plan.gate)
            .unwrap_or_default()
    }

    /// The dev server to start on the thread of `call`, if one is planned and
    /// the call has a browser to open it
    pub(super) fn dev_run(&self, call: &AgentCall) -> Option<DevRun> {
        call.reach.browser.as_ref()?;
        self.planned.get(call.issue)
    }

    /// The current work item's turn is over: its dev server's plan goes
    pub(super) fn dev_server_ended(&self) {
        if let Some(issue) = self.current().map(|item| item.issue) {
            self.planned.hold(issue, None);
        }
    }
}

// The arguments and variables that start `configuration`'s server on `port`, which kelpie
// `picked` when the file's own was taken.
fn command_of(
    configuration: &launch::Configuration,
    port: u16,
    picked: bool,
) -> (Vec<String>, BTreeMap<String, String>) {
    let mut args = configuration.runtime_args.clone();
    let mut env = BTreeMap::from([("NODE_USE_ENV_PROXY".to_owned(), "1".to_owned())]);
    if picked {
        env.insert("PORT".to_owned(), port.to_string());
        let configured = (configuration.port.filter(|&p| p != 0)).map(|p| p.to_string());
        for arg in args
            .iter_mut()
            .filter(|a| Some(&**a) == configured.as_ref())
        {
            *arg = port.to_string();
        }
    }
    (args, env)
}

/// A dev server that answered, and the browser's right to open its port
///
/// The port shuts, and then the server stops, when this is dropped.
pub(super) struct Served {
    _started: Started,
    gate: DevPort,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.gate.shut();
    }
}

/// Starts `run` on the call's thread, and says what the worker should know
/// if it does not answer
///
/// The server stops when the returned [`Started`] is dropped. A server that
/// cannot be started or never answers leaves the turn running, a note and a
/// line at the end of the call's instructions saying so. Either counts as a
/// failed run of the dev server, and a server that answers within
/// [`READY_WITHIN`] as an `ok` one.
pub(super) fn started(
    servers: &dyn DevServers,
    run: &DevRun,
    call: &mut AgentCall,
    notes: &Notes,
    metrics: &Recorder,
) -> Option<Served> {
    let log = run.log.display();
    let served = |started| Served {
        _started: started,
        gate: run.gate.clone(),
    };
    let (started, line) = match servers.start(run) {
        Ok(started) if started.answered => {
            metrics.feature_ran(Feature::DevServer, "ok");
            run.gate.open(run.port);
            return Some(served(started));
        }
        Ok(started) => (
            Some(served(started)),
            format!(
                "The dev server on port {} had not answered after {} seconds: it may still be \
                 starting, or it failed. The browser does not open it this turn. Its output is \
                 in `{log}`.",
                run.port,
                run.ready_within.as_secs()
            ),
        ),
        Err(why) => (
            None,
            format!("Kelpie could not start the dev server: {why}. The turn runs without it."),
        ),
    };
    metrics.feature_ran(Feature::DevServer, "failed");
    notes.push(format!("{}: {line}", who(call)));
    // A resumed session keeps the instructions it began with, so it is told in the prompt.
    match (&call.session, call.instructions.as_deref()) {
        (Session::Resume(_), _) | (_, None) => {
            if !call.prompt.contains(&line) {
                call.prompt = format!("{}\n\n{line}", call.prompt);
            }
        }
        (Session::New(_), Some(instructions)) => append(instructions, &line),
    }
    started
}

// Adds `line` to the instructions' end, for a harness that reads them as it runs.
fn append(file: &Path, line: &str) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    if text.contains(line) {
        return;
    }
    let lead = if text.ends_with('\n') { "" } else { "\n" };
    let Ok(mut open) = std::fs::OpenOptions::new().append(true).open(file) else {
        return;
    };
    let _ = write!(open, "{lead}\n{line}\n");
}
