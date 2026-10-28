//! A Codex reviewer and retro: a shell that reads the repo
//!
//! Codex has no read tool, only a shell, so a reviewer or retro in a
//! fence keeps `shell_tool` and runs `kelpie guard --read-only` on each
//! command: the short list of commands that read, and no redirect. A reviewer
//! with a write folder still patches that folder alone. A call with no fence
//! has no shell.

use super::*;
use crate::guard::{READ_ONLY_COMMANDS, READ_ONLY_FLAG};

/// Whether the call reads through the read-only shell: a reviewer or retro
/// with a fence to hold it
pub(super) fn has_shell(call: &AgentCall) -> bool {
    call.reach.fence.is_some() && judges_reads(call.tools)
}

/// Whether the shell of a call that runs `tools` is the read-only guard's
pub(super) fn judges_reads(tools: Tools) -> bool {
    matches!(tools, Tools::Review | Tools::Retro)
}

/// Whether a call that runs `tools` in `fence` patches no file
///
/// A reviewer asked for tests patches the one folder its fence writes. A
/// retro runs under a fence that can write, which a patch must not use.
pub(super) fn patches_nothing(tools: Tools, fence: &Fence) -> bool {
    match tools {
        Tools::Retro => true,
        Tools::Review => fence.write.is_empty(),
        _ => false,
    }
}

/// Whether the OS sandbox lets the call write nothing but Codex's own home
/// and scratch, whatever its fence writes
pub(super) fn writes_nothing(tools: Tools) -> bool {
    tools == Tools::Retro
}

/// The `Bash` hook's command: `kelpie guard` in its read-only mode, and the
/// project's `command` hooks
pub(super) fn guard_command(fence: &Fence) -> String {
    let guard = &fence.guard;
    let quoted = |p: &Path| shell_quote(&p.to_string_lossy());
    [
        quoted(&guard.kelpie),
        shell_quote("guard"),
        quoted(&guard.git_common_dir),
        quoted(&guard.worktree),
        shell_quote(READ_ONLY_FLAG),
    ]
    .into_iter()
    .chain(guard.hooks.flag().map(|flag| shell_quote(&flag)))
    .collect::<Vec<_>>()
    .join(" ")
}

/// `prompt` for a reviewer, with what its shell is for
///
/// A reviewer's prompt sends it to Read, Glob and Grep, which Codex lacks.
pub(super) fn told(call: &AgentCall, prompt: String) -> String {
    if call.tools != Tools::Review || !has_shell(call) {
        return prompt;
    }
    let commands: Vec<String> = READ_ONLY_COMMANDS
        .iter()
        .map(|c| format!("`{c}`"))
        .collect();
    format!(
        "{prompt}\n\nThis session has no Read, Glob or Grep tool: read the repo with its shell \
         instead. It runs only {}, each as plain words, and pipes between them. It refuses a \
         redirect, a `$` or a backtick, and any other command, git included. Where the \
         instructions above say to run no command, they mean no other.",
        commands.join(", ")
    )
}
