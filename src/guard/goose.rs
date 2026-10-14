//! `kelpie guard ... --goose=<build folder>`: the `PreToolUse` hook of a goose
//! worker
//!
//! goose runs the hook for every tool call and gives it `tool_name`,
//! `tool_input` and `working_dir`. The hook is the worker's only gate, since
//! goose in `auto` mode ignores its own permissions. A shell command goes to
//! the command guard as Claude Code's `Bash` would, and a `write` or an
//! `edit` goes to `kelpie confine` as a `Write` or an `Edit`, held to the
//! worktree and the build folder, with the project's `write` hooks seeing the
//! text. A tree is judged as the `ls` of its folder. Any other tool is
//! refused, and so is a payload this cannot read, a name it does not know
//! being no reason to let a call by.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Checkout, SHOWN_CHARS};
use crate::adapters::goose::hooks::TOOLS;
use crate::adapters::shell_quote;
use crate::confine::{self, Verdict};
use crate::hooks::Hooks;
use crate::local_paths::LocalPaths;

/// How the hook's command line names the worker's build folder, and asks for
/// the goose worker's `PreToolUse` guard
pub const GOOSE_FLAG: &str = "--goose=";

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

/// Judges goose's `PreToolUse` payload in `input`
///
/// `home` and `local` are what the command guard keeps off the forge, and
/// `hooks` the project's own. A file tool is held to `checkout`'s worktree and
/// `build`.
pub fn judge_goose(
    input: impl Read,
    home: Option<&Path>,
    local: LocalPaths,
    hooks: &Hooks,
    checkout: Checkout<'_>,
    build: &Path,
) -> Verdict {
    #[derive(Deserialize)]
    struct Call {
        tool_name: String,
        #[serde(default)]
        tool_input: Value,
        working_dir: PathBuf,
    }
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
            let (name, key, field) = match tool {
                "write" => ("Write", "content", "content"),
                _ => ("Edit", "after", "new_string"),
            };
            let (Some(path), Some(content)) = (text("path"), text(key)) else {
                return unreadable();
            };
            let file = json!({
                "tool_name": name,
                "cwd": cwd,
                "tool_input": { "file_path": path, field: content },
            });
            let folders = [checkout.worktree.to_owned(), build.to_owned()];
            confine::judge_with(file.to_string().as_bytes(), &folders, hooks)
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
        tool if TOOLS.contains(&tool) => Verdict::Allow,
        tool => {
            let shown: String = tool.chars().take(SHOWN_CHARS).collect();
            Verdict::Refuse(format!(
                "kelpie lets a goose worker use only these tools: {}. `{shown}` is not one of them",
                TOOLS.map(|t| format!("`{t}`")).join(", ")
            ))
        }
    }
}
