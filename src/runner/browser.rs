//! A browser outside the worker's sandbox for the turns that look at the UI
//!
//! A project that turns it on in `browser` has each worker turn given the
//! Playwright MCP server, behind a socket kelpie filters. The bridge opens
//! when the turn is prepared and lives with the turn's flight, so a handover
//! cannot carry it ([`Runner::carry_calls`] refuses while one is open).

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use super::Runner;
use super::code_graph::who;
use crate::adapters::bridge::{Bridge, Server};
use crate::bridge::Filter;
use crate::metrics::Feature;
use crate::ports::{AgentCall, Browser, Role, Session, Tools};
use crate::settings::{AgentHarness, NonBlank};
use crate::ticket::TicketId;

/// The bridges opened for turns not yet launched, by the ticket of their work item
#[derive(Debug, Default)]
pub(super) struct Opened(Mutex<BTreeMap<TicketId, Bridge>>);

impl Opened {
    /// Takes the bridge opened for `ticket`'s turn, which the turn's flight then holds
    pub(super) fn take(&self, ticket: TicketId) -> Option<Bridge> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(&ticket)
    }

    // A bridge already held for `ticket` is dropped, and its socket removed.
    fn hold(&self, ticket: TicketId, bridge: Option<Bridge>) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match bridge {
            Some(bridge) => held.insert(ticket, bridge),
            None => held.remove(&ticket),
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
        ticket: Option<TicketId>,
    ) -> Option<&[NonBlank]> {
        self.may_browse(harness, session, ticket)
            .then_some(self.settings.browser.domains.as_slice())
            .filter(|_| self.paths.tools.has_browser())
    }

    fn may_browse(
        &self,
        harness: &AgentHarness,
        session: &Session,
        ticket: Option<TicketId>,
    ) -> bool {
        // parity: harness.browser
        let loads = harness.drives_a_browser();
        // Chromium outside the sandbox is a macOS measurement; Linux waits on #135.
        let here = cfg!(any(target_os = "macos", test));
        self.settings.browser.enabled
            && loads
            && here
            && self.session_has_it(harness, session, ticket)
    }

    // A session that keeps the servers it began with has the browser only if it began with one:
    // kelpie noted its id as it opened the browser for the session's first call.
    // A call with no ticket of its own has no folder to note one in.
    fn session_has_it(
        &self,
        harness: &AgentHarness,
        session: &Session,
        ticket: Option<TicketId>,
    ) -> bool {
        match session {
            Session::Resume(id) if harness.keeps_its_servers() => {
                let noted = (ticket)
                    .map(|t| std::fs::read_to_string(self.paths.browser(t).join(SESSION_FILE)));
                noted.is_some_and(|noted| noted.is_ok_and(|noted| noted.trim() == id.0))
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
        let ticket = ticket_of(call);
        if !turn || !self.may_browse(&call.harness, &call.session, ticket) {
            return Ok(());
        }
        let ticket = ticket.expect("checked above");
        // A bridge opened for an earlier prepare of this work item's turn goes.
        self.opened.hold(ticket, None);
        let opened = self.open_browser(call, ticket);
        let outcome = match opened {
            Ok(true) => "ok",
            Ok(false) => "skipped",
            Err(_) => "failed",
        };
        self.metrics.feature_ran(Feature::Browser, outcome);
        opened.map(drop)
    }

    // Whether the bridge was opened: not when the tools are missing.
    fn open_browser(&self, call: &mut AgentCall, ticket: TicketId) -> Result<bool, String> {
        let tools = &self.paths.tools;
        if !tools.has_browser() {
            self.notes.push(format!(
                "{} runs without a browser: {} or Chromium is not there, so run \
                 `shep kelpie tools install`",
                who(call),
                tools.playwright_mcp().display()
            ));
            return Ok(false);
        }
        let folder = self.paths.browser(ticket);
        let server = Server::playwright(
            &tools.playwright_mcp(),
            &tools.browsers(),
            &folder,
            self.settings.browser.headed,
        )?;
        if let (Session::New(id), true) = (&call.session, call.harness.keeps_its_servers()) {
            std::fs::write(folder.join(SESSION_FILE), &id.0)
                .map_err(|e| format!("cannot note the session of {}: {e}", who(call)))?;
        }
        let filter = Filter {
            domains: (self.settings.browser.domains.iter())
                .map(|d| d.as_str().to_owned())
                .collect(),
            dev_port: self.dev_gate(ticket),
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
        self.opened.hold(ticket, Some(bridge));
        Ok(true)
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
            if let Some(ticket) = ticket_of(call) {
                drop(self.opened.take(ticket));
            }
        }
        opened.and(written)
    }
}

// The ticket of the work item `call` is on: its issue is the GitHub number of
// a GitHub ticket, and a local ticket's call carries none, so its worker runs
// with no browser.
pub(super) fn ticket_of(call: &AgentCall) -> Option<TicketId> {
    TicketId::from_issue(call.issue)
}
