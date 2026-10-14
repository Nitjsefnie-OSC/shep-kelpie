//! A browser outside the worker's sandbox for the turns that look at the UI
//!
//! A project that turns it on in `browser` has each worker turn given the
//! Playwright MCP server, behind a socket kelpie filters. The bridge opens
//! when the turn is prepared and lives with the turn's flight, so a handover
//! cannot carry it ([`Runner::carry_calls`] refuses while one is open).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use super::Runner;
use super::code_graph::who;
use crate::adapters::bridge::{Bridge, Server};
use crate::bridge::Filter;
use crate::ports::{AgentCall, Browser, Role, Session, Tools};
use crate::settings::{AgentHarness, NonBlank};

/// The bridges opened for turns not yet launched, by the issue of their work item
#[derive(Debug, Default)]
pub(super) struct Opened(Mutex<BTreeMap<u64, Bridge>>);

impl Opened {
    /// Takes the bridge opened for `issue`'s turn, which the turn's flight then holds
    pub(super) fn take(&self, issue: u64) -> Option<Bridge> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(&issue)
    }

    // A bridge already held for `issue` is dropped, and its socket removed.
    fn hold(&self, issue: u64, bridge: Option<Bridge>) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match bridge {
            Some(bridge) => held.insert(issue, bridge),
            None => held.remove(&issue),
        };
    }
}

/// The file in a work item's browser folder naming the session its browser was opened for
const SESSION_FILE: &str = "session";

impl Runner {
    /// The hosts a turn on `harness` is told it can open, if it gets the browser
    ///
    /// A turn gets it when the project turned it on, the harness can load
    /// it, this is macOS and the tools are installed. `None` otherwise.
    pub(super) fn browser_hosts(
        &self,
        harness: &AgentHarness,
        session: &Session,
        issue: u64,
    ) -> Option<&[NonBlank]> {
        self.may_browse(harness, session, issue)
            .then_some(self.settings.browser.domains.as_slice())
            .filter(|_| self.paths.tools.has_browser())
    }

    fn may_browse(&self, harness: &AgentHarness, session: &Session, issue: u64) -> bool {
        // parity: harness.browser
        let loads = harness.drives_a_browser();
        // Chromium outside the sandbox is a macOS measurement; Linux waits on #135.
        let here = cfg!(any(target_os = "macos", test));
        self.settings.browser.enabled
            && loads
            && here
            && self.session_has_it(harness, session, issue)
    }

    // A session that keeps the servers it began with has the browser only if it began with one:
    // kelpie noted its id as it opened the browser for the session's first call.
    fn session_has_it(&self, harness: &AgentHarness, session: &Session, issue: u64) -> bool {
        match session {
            Session::Resume(id) if harness.keeps_its_servers() => {
                let noted = std::fs::read_to_string(self.paths.browser(issue).join(SESSION_FILE));
                noted.is_ok_and(|noted| noted.trim() == id.0)
            }
            Session::New(_) | Session::Resume(_) => true,
        }
    }

    /// Opens the browser for `call` and names its socket in the call's reach,
    /// if `call` is a worker's turn the project gives one
    ///
    /// A call that cannot have it because the tools are missing runs without,
    /// and a note says why.
    ///
    /// # Errors
    ///
    /// Why the bridge cannot be opened, which fails the call rather than run
    /// it without a browser the worker's instructions promise.
    pub(super) fn with_browser(&self, call: &mut AgentCall) -> Result<(), String> {
        let turn = (call.role, call.tools) == (Role::Worker, Tools::Work)
            && !call.prompt.is_empty()
            && call.prompt != crate::ports::COMPACT;
        if !turn || !self.may_browse(&call.harness, &call.session, call.issue) {
            return Ok(());
        }
        // A bridge opened for an earlier prepare of this work item's turn goes.
        self.opened.hold(call.issue, None);
        let tools = &self.paths.tools;
        if !tools.has_browser() {
            self.notes.push(format!(
                "{} runs without a browser: {} or Chromium is not there, so run \
                 `shep kelpie tools install`",
                who(call),
                tools.playwright_mcp().display()
            ));
            return Ok(());
        }
        let folder = self.paths.browser(call.issue);
        let server = server(&self.settings.browser, &self.paths.tools, &folder)?;
        if let (Session::New(id), true) = (&call.session, call.harness.keeps_its_servers()) {
            std::fs::write(folder.join(SESSION_FILE), &id.0)
                .map_err(|e| format!("cannot note the session of {}: {e}", who(call)))?;
        }
        let filter = Filter {
            domains: (self.settings.browser.domains.iter())
                .map(|d| d.as_str().to_owned())
                .collect(),
        };
        let bridge = Bridge::open(
            &self.paths.worker,
            &folder.join("socket-name"),
            server,
            filter,
        )
        .map_err(|e| format!("cannot open the browser for {}: {e}", who(call)))?;
        call.reach.browser = Some(Box::new(Browser {
            kelpie: self.kelpie.clone(),
            socket: bridge.socket().to_owned(),
        }));
        self.opened.hold(call.issue, Some(bridge));
        Ok(())
    }

    /// Opens a new browser for a call that had one and is run again
    ///
    /// The bridge of the call's first flight ended with it, and the call's MCP
    /// config on disk still names that socket, so the config is written again.
    ///
    /// # Errors
    ///
    /// Why the browser cannot be opened or the config written. The call then
    /// runs without a browser, or with the config it has.
    pub(super) fn reopen_browser(&self, call: &mut AgentCall) -> Result<(), String> {
        if call.reach.browser.take().is_none() {
            return Ok(());
        }
        let opened = self.with_browser(call);
        let written = (self.ports.agents.prepare(call)).map_err(|e| e.to_string());
        if written.is_err() {
            drop(self.opened.take(call.issue));
        }
        opened.and(written)
    }
}

// The command that serves one connection, with the folder it writes to made
// ready: a real folder of kelpie's own, not a link a worker could have put there.
fn server(
    settings: &crate::settings::Browser,
    tools: &crate::tools::Tools,
    folder: &Path,
) -> Result<Server, String> {
    let output = folder.join("output");
    for dir in [folder, output.as_path()] {
        match std::fs::symlink_metadata(dir) {
            Ok(found) if found.is_dir() => {}
            Ok(_) => return Err(format!("{} is not a folder kelpie made", dir.display())),
            Err(_) => std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot make {}: {e}", dir.display()))?,
        }
    }
    let mut args = vec![
        tools.playwright_mcp().display().to_string(),
        "--isolated".to_owned(),
        "--output-dir".to_owned(),
        output.display().to_string(),
    ];
    if !settings.headed {
        args.push("--headless".to_owned());
    }
    Ok(Server {
        command: "node".into(),
        args,
        env: BTreeMap::from([(
            "PLAYWRIGHT_BROWSERS_PATH".to_owned(),
            tools.browsers().display().to_string(),
        )]),
        cwd: folder.to_owned(),
    })
}
