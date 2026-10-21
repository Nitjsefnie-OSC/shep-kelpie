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

use std::ffi::OsString;

use crate::ports::AgentCall;

/// The `GOOSE_MODE` of the terminal, where goose asks before each tool call
pub(super) const MODE: &str = "approve";

// goose's `--session-id` takes its own id for the session, and kelpie's id is the name
// the session was created with, as for `run`.
pub(super) fn argv(call: &AgentCall) -> Vec<OsString> {
    [
        "session",
        "--resume",
        "--name",
        call.session.id().0.as_str(),
    ]
    .into_iter()
    .map(Into::into)
    .collect()
}
