//! Codex's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same home, scratch folder,
//! sandbox and policy, with the same `-c` overrides, features and hooks, resumed
//! with `codex resume`. Codex's own sandbox stays off, as in the headless call,
//! since it cannot start inside kelpie's, so Codex asks before each command
//! that is not on its list of trusted ones, as Claude Code asks before each
//! tool call. The terminal session and the headless call share the home's
//! `thread-writer-locks`, which attach keeps apart by ending every call first.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{
    AgentCall, AgentError, CODEX, CodexCli, Files, SessionId, Thread, context, instructions,
    is_codex, overrides, prompt, reading, save_thread, thread,
};

/// The settings path of each terminal session, by session id, until its end
///
/// The session's end is told only its id, and the thread record it updates is
/// found from the settings path.
#[derive(Debug, Clone, Default)]
pub(super) struct Terminals(Arc<Mutex<HashMap<String, PathBuf>>>);

impl Terminals {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, PathBuf>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// The session resumed by Codex's own id for it, with the call's prompt as the
// first message if it has one. The interactive command takes no
// `--ignore-user-config` or `--ignore-rules`: the call's home holds no
// `config.toml` or rules for them to skip (`CodexCli::prepare`).
fn terminal_argv(
    call: &AgentCall,
    home: &Path,
    resumed: &Thread,
    instructions: Option<&str>,
) -> Result<Vec<OsString>, AgentError> {
    let mut argv: Vec<OsString> = vec![
        "resume".into(),
        "--model".into(),
        call.model.as_str().into(),
    ];
    argv.extend(
        [
            "--sandbox",
            "danger-full-access",
            "--ask-for-approval",
            "untrusted",
        ]
        .map(OsString::from),
    );
    argv.extend(overrides(call, home, Some(resumed), instructions));
    argv.extend(["--".into(), resumed.id.as_str().into()]);
    if !call.prompt.is_empty() {
        argv.push(reading::told(call, prompt(call)?).into());
    }
    Ok(argv)
}

impl CodexCli {
    pub(super) fn terminal_command(&self, call: &AgentCall) -> Result<Command, AgentError> {
        is_codex(call)?;
        let files = Files::of(call);
        let id = call.session.id();
        let Some(resumed) = thread(&files, id)? else {
            return Err(AgentError::NoSession(CODEX, id.clone()));
        };
        let instructions = instructions(call)?;
        let argv = terminal_argv(call, &files.home, &resumed, instructions.as_deref())?;
        let command = self.in_sandbox(call, &files, argv)?;
        self.terminals
            .lock()
            .insert(id.0.clone(), call.settings.clone());
        Ok(command)
    }

    // The terminal session ran on the session's model requests, so the record
    // takes the tokens and context it ended on. A headless turn after it then
    // counts only its own, since Codex's report runs on from the session's
    // total. A rollout that cannot be read leaves the record as it was.
    pub(super) fn terminal_ended(&self, session: &SessionId) {
        let Some(settings) = self.terminals.lock().remove(&session.0) else {
            return;
        };
        let files = Files::at(&settings);
        let Ok(Some(known)) = thread(&files, session) else {
            return;
        };
        let Some((spent, context)) = context::running_total(&files.home, &known.id)
        else {
            return;
        };
        let record = Thread {
            spent,
            context: Some(context),
            // The session compacted at its first request if the record asked.
            compact: false,
            ..known
        };
        if let Err(e) = save_thread(&files, session, &record) {
            eprintln!(
                "cannot save what the terminal session of {} spent: {e}",
                session.0
            );
        }
    }
}
