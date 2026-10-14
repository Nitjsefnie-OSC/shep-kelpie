//! Codex's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same home, scratch folder,
//! sandbox and policy, with the same `-c` overrides, features and hooks, resumed
//! with `codex resume`. Codex's own sandbox stays off, as in the headless call,
//! since it cannot start inside kelpie's, so Codex asks before each command
//! that is not on its list of trusted ones, as Claude Code asks before each
//! tool call. The terminal session and the headless call share the home's
//! `thread-writer-locks`, which attach keeps apart by ending every call first.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use super::{
    AgentCall, AgentError, CODEX, CodexCli, Files, Thread, instructions, is_codex, overrides,
    prompt, reading, thread,
};

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
        self.in_sandbox(call, &files, argv)
    }
}
