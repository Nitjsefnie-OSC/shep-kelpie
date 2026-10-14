//! `kelpie guard ... --read-only`: the commands of a call that reads the repo
//! and changes nothing
//!
//! A Codex reviewer, advisor or retro has no read tool, only a shell, so its
//! guard runs only what reads: a short list of commands, each as plain words,
//! piped into one another if need be. It refuses a redirect, a `$` or a
//! backtick the shell would expand, a function definition, a command off the
//! list, and a flag that writes a file or runs a program, such as
//! `find -exec` or `rg --pre`. It runs no git: Claude Code's reviewer has
//! none, and a repo's own config can name a program.
//!
//! The project's `command` hooks judge what it lets through, whole.

use std::io::Read;

use serde::Deserialize;

use super::shell::{self, Command};
use super::wrap::FUNCTION;
use super::{MAX_COMMANDS, command_hooks, take_flag};
use crate::confine::Verdict;
use crate::hooks::{Hooks, Role};

/// How the hook's command line asks for the read-only guard
pub const READ_ONLY_FLAG: &str = "--read-only";

/// Whether `args` asks for [`READ_ONLY_FLAG`], and the rest of them
pub fn reading_flag(args: &[String]) -> (bool, Vec<String>) {
    take_flag(args, READ_ONLY_FLAG)
}

/// The commands a reading call may run
pub const READ_ONLY_COMMANDS: [&str; 12] = [
    "cat", "head", "tail", "ls", "tree", "grep", "rg", "find", "wc", "file", "stat", "nl",
];

/// What each listed command's flags may not be: they write a file or run a program
const FLAGS: [(&str, &[&str]); 4] = [
    (
        "find",
        &[
            "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf",
            "-fls",
        ],
    ),
    ("rg", &["--pre", "--hostname-bin"]),
    ("tree", &["-o", "--output"]),
    ("file", &["-C", "--compile"]),
];

fn only() -> String {
    let listed: Vec<String> = READ_ONLY_COMMANDS
        .iter()
        .map(|c| format!("`{c}`"))
        .collect();
    format!(
        "this call runs only these commands, each as plain words: {}. Read the repo with \
         them, and run no git",
        listed.join(", ")
    )
}

const PLAIN: &str = "kelpie runs a command here only as plain words: no redirect (`>`, `<`, \
    `2>&1`), here-string or heredoc. Pipe listed commands into one another instead";

const EXPANDS: &str = "kelpie cannot check a `$` or backtick the shell expands: write each \
    value out, or hold a pattern's `$` in single quotes";

/// Judges the tool call in `input`: a shell command that reads and writes
/// nothing is allowed, and the project's `hooks` judge it too
pub fn judge_reading(input: impl Read, hooks: &Hooks) -> Verdict {
    #[derive(Deserialize)]
    struct Call {
        tool_name: String,
        #[serde(default)]
        cwd: std::path::PathBuf,
        #[serde(default)]
        tool_input: ToolInput,
    }
    #[derive(Deserialize, Default)]
    struct ToolInput {
        command: Option<String>,
    }
    let call: Call = match serde_json::from_reader(input) {
        Ok(call) => call,
        Err(e) => return Verdict::Refuse(format!("kelpie cannot read this tool call: {e}")),
    };
    if call.tool_name != "Bash" {
        return Verdict::Allow;
    }
    let line = call.tool_input.command.unwrap_or_default();
    let commands = match shell::commands(&line) {
        Ok(commands) if commands.len() <= MAX_COMMANDS => commands,
        Ok(_) => return Verdict::Refuse(format!("{}.", only())),
        Err(e) => return Verdict::Refuse(format!("kelpie cannot check this command: {e}.")),
    };
    let mut refusals: Vec<String> = Vec::new();
    for command in &commands {
        if let Err(why) = judge(command)
            && !refusals.contains(&why)
        {
            refusals.push(why);
        }
    }
    match refusals.is_empty() {
        true => command_hooks(hooks, Role::Worker, &line, &call.cwd, None),
        false => Verdict::Refuse(refusals.join("\n\n")),
    }
}

fn judge(command: &Command) -> Result<(), String> {
    if command.defines_function {
        return Err(FUNCTION.into());
    }
    let (name, args) = command.words.split_first().ok_or_else(only)?;
    if !READ_ONLY_COMMANDS.contains(&name.as_str()) {
        return Err(format!("{}.", only()));
    }
    if (command.words.iter().zip(&command.quoting)).any(|(w, q)| holds(w, &q.quoted, ['<', '>'])) {
        return Err(format!("{PLAIN}."));
    }
    if (command.words.iter().zip(&command.quoting)).any(|(w, q)| holds(w, &q.quoted, ['$', '`'])) {
        return Err(format!("{EXPANDS}."));
    }
    let flags = FLAGS
        .iter()
        .find(|(n, _)| n == name)
        .map_or(&[][..], |f| f.1);
    match args.iter().find_map(|a| refused_flag(a, flags)) {
        Some(flag) => Err(format!(
            "`{name} {flag}` writes a file or runs a program, so it is refused: leave it out."
        )),
        None => Ok(()),
    }
}

// Whether `word` holds one of `chars` the shell reads as syntax: one no
// quote or backslash holds literal.
fn holds(word: &str, quoted: &[bool], chars: [char; 2]) -> bool {
    word.chars()
        .zip(quoted)
        .any(|(c, quoted)| chars.contains(&c) && !quoted)
}

// The flag in `flags` that `arg` is, or holds in a cluster of short flags.
fn refused_flag<'a>(arg: &str, flags: &[&'a str]) -> Option<&'a str> {
    flags
        .iter()
        .copied()
        .find(|flag| match flag.strip_prefix("--") {
            Some(_) => arg == *flag || arg.starts_with(&format!("{flag}=")),
            // `find`'s one-dash flags are whole words; a short flag is a letter in a cluster.
            None if flag.len() == 2 => {
                arg.starts_with('-') && !arg.starts_with("--") && arg.contains(&flag[1..])
            }
            None => arg == *flag,
        })
}

#[cfg(test)]
mod tests;
