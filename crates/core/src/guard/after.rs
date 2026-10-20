//! `kelpie guard --after`: the PostToolUse hook that runs the project's
//! `after_command` hooks on a shell command that has run
//!
//! The command cannot be undone, so a hook's refusal reaches the agent as
//! feedback on the result: the hook's exit 2 and stderr, which Claude Code and
//! Codex hand to the model after the tool call, and pi's `tool_result` handler
//! appends. Each harness words its tool result its own way, so the exit and the
//! output are read from whichever of the known shapes is there.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::confine::Verdict;
use crate::hooks::{Hooks, Role};
use crate::settings::HookPoint;

/// How the hook's command line asks for the PostToolUse hook that runs
/// `after_command` hooks, in place of the PreToolUse one
pub const AFTER_FLAG: &str = "--after";

// The most of a command's output a hook is given, in characters from its end.
const TAIL: usize = 4000;

/// Whether `args` asks for [`AFTER_FLAG`], and the rest of them
pub fn after_flag(args: &[String]) -> (bool, Vec<String>) {
    super::take_flag(args, AFTER_FLAG)
}

/// Runs the project's `after_command` hooks on the finished shell command in
/// `input`, for `role`, in `worktree`
pub fn judge_after(input: impl Read, hooks: &Hooks, role: Role, worktree: &Path) -> Verdict {
    #[derive(Deserialize)]
    struct Call {
        tool_name: String,
        cwd: PathBuf,
        #[serde(default)]
        tool_input: Value,
        #[serde(default)]
        tool_response: Value,
        #[serde(default)]
        error: Option<String>,
    }
    let call: Call = match serde_json::from_reader(input) {
        Ok(call) => call,
        Err(e) => return Verdict::Refuse(format!("kelpie cannot read this tool result: {e}")),
    };
    let Some(line) = call.tool_input["command"]
        .as_str()
        .filter(|_| call.tool_name == "Bash" && hooks.has(HookPoint::AfterCommand))
    else {
        return Verdict::Allow;
    };
    let (exit, output) = result(&call.tool_response, call.error.as_deref());
    let details = json!({ "command": line, "cwd": call.cwd, "exit": exit, "output": output });
    // The issue writer's hooks run where its commands do, as its `command` hooks do.
    let folder = (role == Role::Worker).then_some(worktree);
    match hooks.check(HookPoint::AfterCommand, role, folder, details) {
        Ok(()) => Verdict::Allow,
        Err(refusal) => Verdict::Refuse(format!("after this command: {refusal}")),
    }
}

// The exit and the tail of the output, from a tool result in any of the shapes
// the harnesses give. An exit nobody states is 0 for a result and null for an
// error that names none.
fn result(response: &Value, error: Option<&str>) -> (Value, String) {
    let number = |keys: &[&str]| keys.iter().find_map(|k| response[k].as_i64());
    let text =
        |keys: &[&str]| -> Vec<&str> { keys.iter().filter_map(|k| response[k].as_str()).collect() };
    let mut parts = text(&["stdout", "output", "content"]);
    parts.extend(text(&["stderr"]));
    if let Some(whole) = response.as_str() {
        parts.push(whole);
    }
    let said = error.map(str::to_owned);
    // Claude Code's failure event says `Exit code N` first, then the output.
    let (stated, rest) = said
        .as_deref()
        .map_or((None, None), |e| match exit_code(e) {
            Some((code, rest)) => (Some(code), Some(rest)),
            None => (None, Some(e)),
        });
    parts.extend(rest);
    let exit = number(&["exit_code", "exitCode", "exit", "returncode"])
        .or(stated)
        .map_or_else(
            || match error.is_some() {
                true => Value::Null,
                false => json!(0),
            },
            |code| json!(code),
        );
    let joined = parts
        .iter()
        .map(|p| p.trim_end())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let skip = joined.chars().count().saturating_sub(TAIL);
    (exit, joined.chars().skip(skip).collect())
}

// `Exit code 101\nerror...` as the code and the rest.
fn exit_code(error: &str) -> Option<(i64, &str)> {
    let after = error.strip_prefix("Exit code ")?;
    let (code, rest) = after.split_once('\n').unwrap_or((after, ""));
    Some((code.trim().parse().ok()?, rest))
}
