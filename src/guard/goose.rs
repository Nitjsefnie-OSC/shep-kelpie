//! `kelpie guard ... --goose=<build folder>`: the `PreToolUse` hook of a goose
//! worker
//!
//! goose runs the hook for every tool call and gives it `tool_name`,
//! `tool_input` and `working_dir`. The hook is the worker's only gate, since
//! goose in `auto` mode ignores its own permissions. A shell command goes to
//! the command guard as Claude Code's `Bash` would, and a `write` or an
//! `edit` goes to `kelpie confine` as a `Write` or an `Edit`, held to the
//! worktree and the build folder, with the project's `write` hooks seeing the
//! text. A tree is judged as the `ls` of its folder. A tool of an MCP server the
//! call loads is let through, the sandbox being the boundary for a server. Any
//! other tool is refused, and so is a payload this cannot read, a name it does not know
//! being no reason to let a call by.
//!
//! A goose reviewer, advisor or retro has the read-only guard instead,
//! [`judge_goose_reading`]: its `shell` runs only what [`judge_reading`] lists,
//! its `tree` is the `ls` of its folder, and every other tool is refused, since
//! the call writes no file. A reviewer asked for tests has [`judge_goose_writing`]:
//! the same, and its `write` goes to `kelpie confine` held to its tests folder.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Checkout, SHOWN_CHARS, judge_reading};
use crate::adapters::goose::hooks::{SKILL_TOOL, TOOLS};
use crate::adapters::shell_quote;
use crate::confine::{self, Verdict};
use crate::hooks::Hooks;
use crate::local_paths::LocalPaths;

/// How the hook's command line names the worker's build folder, and asks for
/// the goose worker's `PreToolUse` guard
pub const GOOSE_FLAG: &str = "--goose=";

/// How the read-only hook of a goose reviewer asked for tests names the one
/// folder its `write` tool may write
pub const GOOSE_WRITE_FLAG: &str = "--goose-write=";

/// The folder `args` name for [`GOOSE_WRITE_FLAG`], and the rest of them
pub fn goose_write_flag(args: &[String]) -> (Option<PathBuf>, Vec<String>) {
    let mut folder = None;
    let mut rest = Vec::new();
    for arg in args {
        match arg.strip_prefix(GOOSE_WRITE_FLAG) {
            Some(path) => folder = Some(PathBuf::from(path)),
            None => rest.push(arg.clone()),
        }
    }
    (folder, rest)
}

/// How the hook's command line names an MCP server whose tools it lets through, once for each
pub const GOOSE_SERVER_FLAG: &str = "--goose-server=";

/// The servers `args` name for [`GOOSE_SERVER_FLAG`], and the rest of them
pub fn goose_server_flags(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut servers = Vec::new();
    let mut rest = Vec::new();
    for arg in args {
        match arg.strip_prefix(GOOSE_SERVER_FLAG) {
            Some(key) => servers.push(key.to_owned()),
            None => rest.push(arg.clone()),
        }
    }
    (servers, rest)
}

/// The build folder `args` names for [`GOOSE_FLAG`], and the rest of them
pub fn goose_flag(args: &[String]) -> (Option<PathBuf>, Vec<String>) {
    let mut build = None;
    let mut rest = Vec::new();
    for arg in args {
        match arg.strip_prefix(GOOSE_FLAG) {
            Some(folder) => build = Some(PathBuf::from(folder)),
            None => rest.push(arg.clone()),
        }
    }
    (build, rest)
}

// goose's `PreToolUse` payload
#[derive(Deserialize)]
struct Call {
    tool_name: String,
    #[serde(default)]
    tool_input: Value,
    working_dir: PathBuf,
}

// A `write` or an `edit` of `call` as Claude Code's `Write` or `Edit`, judged by
// `kelpie confine` against `folders`. goose resolves the path as `confine` does:
// an absolute path as it is, a relative one from the call's working folder, and
// a `~` not at all.
fn judge_file(call: &Call, tool: &str, hooks: &Hooks, folders: &[PathBuf]) -> Verdict {
    let (name, key, field) = match tool {
        "write" => ("Write", "content", "content"),
        _ => ("Edit", "after", "new_string"),
    };
    let text = |key: &str| call.tool_input[key].as_str();
    let (Some(path), Some(content)) = (text("path"), text(key)) else {
        return Verdict::Refuse(format!("kelpie cannot read this {} call", call.tool_name));
    };
    let file = json!({
        "tool_name": name,
        "cwd": call.working_dir,
        "tool_input": { "file_path": path, field: content },
    });
    confine::judge_with(file.to_string().as_bytes(), folders, hooks)
}

/// Judges goose's `PreToolUse` payload in `input`
///
/// `home` and `local` are what the command guard keeps off the forge, and
/// `hooks` the project's own. A file tool is held to `checkout`'s worktree and
/// `build`. A tool named `<key>__<tool>` for a `key` in `servers` is an MCP server's, which the
/// sandbox holds and this lets through.
pub fn judge_goose(
    input: impl Read,
    home: Option<&Path>,
    local: LocalPaths,
    hooks: &Hooks,
    checkout: Checkout<'_>,
    build: &Path,
    servers: &[String],
) -> Verdict {
    let call: Call = match serde_json::from_reader(input) {
        Ok(call) => call,
        Err(e) => return Verdict::Refuse(format!("kelpie cannot read this tool call: {e}")),
    };
    let unreadable = || Verdict::Refuse(format!("kelpie cannot read this {} call", call.tool_name));
    let text = |key: &str| call.tool_input[key].as_str();
    let cwd = &call.working_dir;
    match call.tool_name.as_str() {
        "shell" => {
            let Some(command) = text("command") else {
                return unreadable();
            };
            let bash = json!({
                "tool_name": "Bash",
                "cwd": cwd,
                "tool_input": { "command": command },
            });
            super::judge_with(bash.to_string().as_bytes(), home, local, hooks, checkout)
        }
        tool @ ("write" | "edit") => {
            let folders = [checkout.worktree.to_owned(), build.to_owned()];
            judge_file(&call, tool, hooks, &folders)
        }
        "tree" => {
            // A tree lists a folder, so the command guard judges it as the `ls`
            // of that folder, with the same say over paths a `shell` has.
            let path = match &call.tool_input["path"] {
                Value::Null => ".",
                Value::String(path) => path,
                _ => return unreadable(),
            };
            let bash = json!({
                "tool_name": "Bash",
                "cwd": cwd,
                "tool_input": { "command": format!("ls -- {}", shell_quote(path)) },
            });
            super::judge_with(bash.to_string().as_bytes(), home, local, hooks, checkout)
        }
        tool if TOOLS.contains(&tool) || tool == SKILL_TOOL => Verdict::Allow,
        tool if servers.iter().any(|key| of_server(tool, key)) => Verdict::Allow,
        tool => {
            let shown: String = tool.chars().take(SHOWN_CHARS).collect();
            Verdict::Refuse(format!(
                "kelpie lets a goose worker use only these tools: {}. `{shown}` is not one of them",
                [SKILL_TOOL]
                    .iter()
                    .chain(&TOOLS)
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }
}

// Whether `tool` is named `<key>__<tool>`, as goose names the tools of the server `key`.
fn of_server(tool: &str, key: &str) -> bool {
    tool.strip_prefix(key)
        .is_some_and(|rest| rest.starts_with("__") && rest.len() > 2)
}

/// Judges goose's `PreToolUse` payload in `input` for a call that writes no
/// file: a reviewer, an advisor or a retro
///
/// A `shell` command goes to [`judge_reading`] as Claude Code's `Bash` would,
/// so it runs only what reads, and the project's `command` hooks judge it. A
/// `tree` is the `ls` of its folder, judged the same way. Every other tool is
/// refused, `write` and `edit` included, and so is a payload this cannot read. A tool named
/// `<key>__<tool>` for a `key` in `servers` is an MCP server's, which goes through as in
/// [`judge_goose`].
pub fn judge_goose_reading(input: impl Read, hooks: &Hooks, servers: &[String]) -> Verdict {
    judge_reading_with(input, hooks, servers, None)
}

/// Judges goose's `PreToolUse` payload in `input` for a reviewer asked for tests
///
/// As [`judge_goose_reading`], except that a `write` goes to `kelpie confine`
/// with `folder` as both its worktree and its build folder, so it can write
/// that folder and nothing else, and the project's `write` hooks see its text.
/// An `edit` and every other tool stay refused.
pub fn judge_goose_writing(
    input: impl Read,
    hooks: &Hooks,
    servers: &[String],
    folder: &Path,
) -> Verdict {
    judge_reading_with(input, hooks, servers, Some(folder))
}

fn judge_reading_with(
    input: impl Read,
    hooks: &Hooks,
    servers: &[String],
    writes: Option<&Path>,
) -> Verdict {
    let call: Call = match serde_json::from_reader(input) {
        Ok(call) => call,
        Err(e) => return Verdict::Refuse(format!("kelpie cannot read this tool call: {e}")),
    };
    let command = match (call.tool_name.as_str(), writes) {
        ("shell", _) => match call.tool_input["command"].as_str() {
            Some(command) => command.to_owned(),
            None => {
                return Verdict::Refuse(format!("kelpie cannot read this {} call", call.tool_name));
            }
        },
        ("tree", _) => match &call.tool_input["path"] {
            Value::Null => "ls -- .".to_owned(),
            Value::String(path) => format!("ls -- {}", shell_quote(path)),
            _ => return Verdict::Refuse("kelpie cannot read this tree call".to_owned()),
        },
        ("write", Some(folder)) => {
            let folders = [folder.to_owned(), folder.to_owned()];
            return judge_file(&call, "write", hooks, &folders);
        }
        (SKILL_TOOL, _) => return Verdict::Allow,
        (tool, _) if servers.iter().any(|key| of_server(tool, key)) => return Verdict::Allow,
        (tool, _) => {
            let shown: String = tool.chars().take(SHOWN_CHARS).collect();
            return Verdict::Refuse(match writes {
                Some(_) => format!(
                    "this call reads the repo with `shell` and `tree` and writes its tests \
                     folder with `write` alone, and `{shown}` is not one of them"
                ),
                None => format!(
                    "this call writes no file: it reads the repo with `shell` and `tree` \
                     alone, and `{shown}` is not one of them"
                ),
            });
        }
    };
    let bash = json!({
        "tool_name": "Bash",
        "cwd": call.working_dir,
        "tool_input": { "command": command },
    });
    judge_reading(bash.to_string().as_bytes(), hooks)
}
