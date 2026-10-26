//! Codex's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same home, sandbox, policy and
//! hooks, resumed with `codex resume`. Codex's own sandbox stays off, so it asks
//! before each command it cannot tell is safe (`on-request`). Attach keeps the
//! session and the headless call off the home's `thread-writer-locks` by ending
//! every call first. Codex's "Trust this folder?" answer goes to the home's
//! `config.toml`, which the policy refuses, so kelpie writes it there first,
//! outside the sandbox, and removes it after. The headless call passes
//! `--ignore-user-config`, so never reads it.

use std::ffi::OsString;
use std::path::Path;

use super::{
    AgentCall, AgentError, Files, SessionId, Thread, context, overrides, prompt, reading,
    save_thread, thread,
};

const TRUST: &str = "config.toml";

// `config.toml` in `home` saying `cwd` is trusted, in place of any left before.
pub(super) fn trust_folder(home: &Path, cwd: &Path) -> Result<(), AgentError> {
    let mut project = toml::Table::new();
    project.insert("trust_level".into(), "trusted".into());
    let mut projects = toml::Table::new();
    projects.insert(cwd.to_string_lossy().into_owned(), project.into());
    let mut config = toml::Table::new();
    config.insert("projects".into(), projects.into());
    let file = home.join(TRUST);
    std::fs::write(&file, config.to_string())
        .map_err(|e| AgentError::Setup(format!("cannot write {}: {}", file.display(), e.kind())))
}

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
            "on-request",
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
    if let Err(e) = std::fs::remove_file(files.home.join(TRUST))
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("cannot remove the trust of {}: {e}", session.0);
    }
    let Ok(Some(known)) = thread(&files, session) else {
        return;
    };
    let Some((spent, context)) = context::running_total(&files.home, &known.id) else {
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
