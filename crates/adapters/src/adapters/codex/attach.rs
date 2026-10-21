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

use super::{
    AgentCall, AgentError, Files, SessionId, Thread, context, overrides, prompt, reading,
    save_thread, thread,
};

// The session resumed by Codex's own id for it, with the call's prompt as the
// first message if it has one. The interactive command takes no
// `--ignore-user-config` or `--ignore-rules`: the call's home holds no
// `config.toml` or rules for them to skip (`Codex::write_config`).
pub(super) fn terminal_argv(
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

// The terminal session ran on the session's model requests, so the record
// takes the tokens and context it ended on. A headless turn after it then
// counts only its own, since Codex's report runs on from the session's
// total. A rollout that cannot be read leaves the record as it was.
pub(super) fn ended(settings: &Path, session: &SessionId) {
    let files = Files::at(settings);
    let Ok(Some(known)) = thread(&files, session) else {
        return;
    };
    let Some((spent, context)) = context::running_total(&files.home, &known.id, known.spent) else {
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
