//! pi's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same home, tools, guard and
//! sandbox, resumed with `--session`. pi asks before no tool call, so the
//! sandbox and the guard extension are what hold while the maintainer drives.
//! The forwarder outlives [`crate::ports::Agents::foreground`], since the terminal's pi
//! dials it for as long as the session runs, and ends with
//! [`crate::ports::Agents::foreground_ended`]. The session holds no gateway lease: the
//! maintainer may leave it open for hours, and a lease is for a call's span.

use std::collections::HashMap;
use std::ffi::OsString;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::shared::profile;
use super::{Files, PiCli, refuse_missing_session, server, skill_prompt};
use crate::adapters::forwarder::Forwarder;
use crate::ports::{AgentCall, AgentError, SessionId};

/// The forwarder of each terminal session, by session id, until its end
#[derive(Debug, Clone, Default)]
pub(super) struct Terminals(Arc<Mutex<HashMap<String, Forwarder>>>);

impl Terminals {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Forwarder>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// The session resumed, with the call's prompt as the first message if it has one.
fn terminal_argv(call: &AgentCall, files: &Files) -> Vec<OsString> {
    let (prompt, skill) = skill_prompt(call);
    let mut argv = profile(call, files, skill);
    argv.extend(["--session".into(), call.session.id().0.as_str().into()]);
    if !prompt.is_empty() {
        argv.extend(["--".into(), prompt.into()]);
    }
    argv
}

impl PiCli {
    pub(super) fn terminal_command(&self, call: &AgentCall) -> Result<Command, AgentError> {
        refuse_missing_session(call)?;
        let upstream = self.upstream(server(call)?)?;
        let argv = terminal_argv(call, &Files::of(call));
        let (command, forwarder) = self.inside_sandbox(call, upstream, argv)?;
        self.terminals
            .lock()
            .insert(call.session.id().0.clone(), forwarder);
        Ok(command)
    }

    pub(super) fn drop_terminal(&self, session: &SessionId) {
        self.terminals.lock().remove(&session.0);
    }
}
