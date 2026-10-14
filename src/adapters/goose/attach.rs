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
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::output::Spent;
use super::{Files, GOOSE, GooseCli, server};
use crate::adapters::forwarder::Forwarder;
use crate::ports::{AgentCall, AgentError, SessionId};

/// The `GOOSE_MODE` of the terminal, where goose asks before each tool call
const TERMINAL_MODE: &str = "approve";

/// A terminal session until its end: its forwarder
#[derive(Debug)]
struct Terminal {
    _forwarder: Forwarder,
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
        let (command, forwarder) = self.wrapped_command(call, None, argv, Some(TERMINAL_MODE))?;
        // The terminal session runs on the session's model requests, so goose's totals will
        // hold tokens kelpie cannot count: they are in goose's database. The record notes that
        // when the session starts, not when it ends, since the hold outlives a runner that
        // restarts and nothing in memory then remembers the session. The next headless turn
        // counts from the totals it reports.
        let files = Files::of(&call.settings);
        if let Some(known) = files.spent(id)? {
            files.keep(
                id,
                Spent {
                    rebase: true,
                    ..known
                },
            )?;
        }
        let terminal = Terminal {
            _forwarder: forwarder,
        };
        self.terminals.lock().insert(id.0.clone(), terminal);
        Ok(command)
    }

    pub(super) fn terminal_ended(&self, session: &SessionId) {
        self.terminals.lock().remove(&session.0);
    }
}
