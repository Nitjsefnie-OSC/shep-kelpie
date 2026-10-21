//! pi's session in the maintainer's terminal
//!
//! The interactive twin of a headless call: the same home, tools, guard and
//! sandbox, resumed with `--session`. pi asks before no tool call, so the
//! sandbox and the guard extension are what hold while the maintainer drives.

use std::ffi::OsString;

use super::shared::profile;
use super::{Files, skill_prompt};
use crate::ports::AgentCall;

// The session resumed, with the call's prompt as the first message if it has one.
pub(super) fn terminal_argv(call: &AgentCall, files: &Files) -> Vec<OsString> {
    let (prompt, skill) = skill_prompt(call);
    let mut argv = profile(call, files, skill);
    argv.extend(["--session".into(), call.session.id().0.as_str().into()]);
    if !prompt.is_empty() {
        argv.extend(["--".into(), prompt.into()]);
    }
    argv
}
