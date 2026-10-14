//! goose's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same root, env, policy and
//! sandbox, with `goose session --resume` in place of `goose run`. goose keeps
//! its sessions in SQLite with no lock of its own, so what keeps the terminal
//! and a headless call apart is the attach's wait and hold. A resumed session
//! uses the extension set it stored, so the command passes no `--with-*` flag.
//! The `PreToolUse` hook still refuses what a worker may not do, and
//! `GOOSE_MODE` is `approve`, so the maintainer is asked before each tool call
//! as Claude Code asks. The forwarder outlives [`crate::ports::Agents::foreground`],
//! since the terminal's goose dials it for as long as the session runs, and ends
//! with [`crate::ports::Agents::foreground_ended`]. The session holds no gateway
//! lease: the maintainer may leave it open for hours, and a lease is for a call's span.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::output::Spent;
use super::{Files, GOOSE, GooseCli, server};
use crate::adapters::forwarder::Forwarder;
use crate::ports::{AgentCall, AgentError, SessionId};

/// The `GOOSE_MODE` of the terminal, where goose asks before each tool call
const TERMINAL_MODE: &str = "approve";

/// A terminal session until its end: its forwarder, and the settings path its files are beside
#[derive(Debug)]
struct Terminal {
    _forwarder: Forwarder,
    settings: PathBuf,
}

/// The terminal sessions, by session id
#[derive(Debug, Clone, Default)]
pub(super) struct Terminals(Arc<Mutex<HashMap<String, Terminal>>>);

impl Terminals {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Terminal>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl GooseCli {
    pub(super) fn terminal_command(&self, call: &AgentCall) -> Result<Command, AgentError> {
        server(call)?;
        let id = call.session.id();
        // `goose session` has no flag for a first message.
        if !call.prompt.is_empty() {
            return Err(AgentError::Setup(
                "goose cannot start a terminal session with a first message: `goose session` \
                 takes none"
                    .into(),
            ));
        }
        if Files::of(&call.settings).spent(id)?.is_none() {
            return Err(AgentError::NoSession(GOOSE, id.clone()));
        }
        // goose's `--session-id` takes its own id for the session, and kelpie's id is the name
        // the session was created with, as for `run`.
        let argv: Vec<OsString> = ["session", "--resume", "--name", id.0.as_str()]
            .into_iter()
            .map(Into::into)
            .collect();
        let (command, forwarder) = self.wrapped(call, None, argv, Some(TERMINAL_MODE))?;
        let terminal = Terminal {
            _forwarder: forwarder,
            settings: call.settings.clone(),
        };
        self.terminals.lock().insert(id.0.clone(), terminal);
        Ok(command)
    }

    // The terminal session ran on the session's model requests, so goose's totals now hold
    // tokens kelpie cannot count: they are in goose's database. The record notes that, and
    // the next headless turn counts from the totals it reports.
    pub(super) fn terminal_ended(&self, session: &SessionId) {
        let Some(terminal) = self.terminals.lock().remove(&session.0) else {
            return;
        };
        let files = Files::of(&terminal.settings);
        let noted = files.spent(session).and_then(|known| {
            let Some(known) = known else { return Ok(()) };
            files.keep(
                session,
                Spent {
                    rebase: true,
                    ..known
                },
            )
        });
        if let Err(e) = noted {
            eprintln!(
                "cannot note what the terminal session of {} spent: {e}",
                session.0
            );
        }
    }
}
