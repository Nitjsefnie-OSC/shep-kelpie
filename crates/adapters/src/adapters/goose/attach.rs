//! goose's session in the maintainer's terminal
//!
//! The interactive twin of a headless call, with `goose session --resume` in
//! place of `goose run`. goose's SQLite sessions have no lock, so the attach's
//! wait and hold keep the terminal and a headless call apart. A resumed session
//! keeps its stored extensions, so the command passes no `--with-*` flag. The
//! `PreToolUse` hook still applies and `GOOSE_MODE` is `approve`. The forwarder
//! outlives [`crate::ports::Agents::foreground`] and ends with
//! [`crate::ports::Agents::foreground_ended`]. The session holds no gateway
//! lease, which is for a call's span, not hours.

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
