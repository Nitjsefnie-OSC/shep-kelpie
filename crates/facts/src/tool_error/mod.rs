//! What a call's transcript says of each tool call that failed: what made it
//! fail, as a kind
//!
//! A failed result counts once, however many lines of output it holds: one
//! command can print the same error on hundreds of lines. A kind is the first
//! that matches, so a guard refusal of a result that also holds a sandbox
//! error is a guard refusal. Each kind's phrase is read at the head of a line,
//! once the markers a harness or a program write in front of its own error are
//! taken off, so output that quotes a phrase is not that error. Claude Code
//! names the hook that refused inside its refusal text, and kelpie's own hooks
//! are its `guard` and `confine` programs, read as shell words. pi's guard
//! extension marks its refusals with [`GUARD_MARK`] and writes each one as a
//! content block of its own, which is what tells a refusal from output that
//! quotes the mark. `docs/facts/tool-errors.md`

use std::collections::HashMap;

use serde_json::Value;

/// The program kelpie's own hooks run, the root package's binary
///
/// A constant, since this crate's own name is not the binary's.
pub const KELPIE_PROGRAM: &str = "shep-kelpie";

/// What a pi call's guard extension puts at the front of every refusal it makes
pub const GUARD_MARK: &str = "kelpie guard: ";

/// Why a tool call failed, as a series' `kind` label
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// kelpie's `guard` or `confine` refused it
    GuardBlock,
    /// A hook that is not kelpie's refused it
    HookBlock,
    /// The call was never given a tool of that name
    ToolNotGiven,
    /// The call has no skill of that name
    UnknownSkill,
    /// A command outside the call's allow list was refused
    PermissionRefused,
    /// Git could not write a ref of its own
    GitRefLock,
    /// The sandbox or `confine` refused a path under `/tmp/`
    TmpRefused,
    /// A permission rule, the sandbox or `confine` refused a path
    PathRefused,
    /// The command, or a program a tool needs, is not there
    CommandNotFound,
    /// An edit's old text is not in the file, or is there more than once
    EditMiss,
    /// The command ran past its limit
    Timeout,
    /// A call to an MCP server failed for none of the reasons above
    Mcp,
    /// Nothing above matched it: a command that ran and failed
    Other,
}

// The kinds a failed result is read for, in that order. A git ref lock prints
// `Operation not permitted` too, so it is read before the two path kinds.
const IN_ORDER: [Kind; 11] = [
    Kind::ToolNotGiven,
    Kind::UnknownSkill,
    Kind::PermissionRefused,
    Kind::GitRefLock,
    Kind::TmpRefused,
    Kind::PathRefused,
    Kind::CommandNotFound,
    Kind::EditMiss,
    Kind::Timeout,
    Kind::Mcp,
    Kind::Other,
];

impl Kind {
    /// The kind's name in a series' `kind` label
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GuardBlock => "guard-block",
            Self::HookBlock => "hook-block",
            Self::ToolNotGiven => "tool-not-given",
            Self::UnknownSkill => "unknown-skill",
            Self::PermissionRefused => "permission-refused",
            Self::GitRefLock => "git-ref-lock",
            Self::TmpRefused => "tmp-refused",
            Self::PathRefused => "path-refused",
            Self::CommandNotFound => "command-not-found",
            Self::EditMiss => "edit-miss",
            Self::Timeout => "timeout",
            Self::Mcp => "mcp",
            Self::Other => "other",
        }
    }
}

/// One tool call that failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    /// What made it fail
    pub kind: Kind,
    /// The hook that refused it, which Claude Code names in its refusal, and which
    /// a log line carries since no label can hold it
    pub hook: Option<String>,
}

/// The tool calls Claude Code's transcript `text` says failed, from byte `from`
/// on, where the call whose lines begin there made them
pub fn claude(text: &str, from: usize) -> Vec<ToolError> {
    let mut named: HashMap<String, String> = HashMap::new();
    let mut failed = Vec::new();
    for entry in entries(text, from) {
        match entry["type"].as_str() {
            Some("assistant") => {
                for block in blocks(&entry["message"]["content"], "tool_use") {
                    if let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                        named.insert(id.to_owned(), name.to_owned());
                    }
                }
            }
            Some("user") => {
                for block in blocks(&entry["message"]["content"], "tool_result") {
                    if block["is_error"].as_bool() != Some(true) {
                        continue;
                    }
                    let tool = (block["tool_use_id"].as_str())
                        .and_then(|id| named.get(id))
                        .map(String::as_str);
                    failed.push(claude_one(&text_of(&block["content"]), tool));
                }
            }
            _ => {}
        }
    }
    failed
}

/// The tool calls pi's session file `text` says failed, from byte `from` on
pub fn pi(text: &str, from: usize) -> Vec<ToolError> {
    let mut failed = Vec::new();
    for entry in entries(text, from) {
        let message = &entry["message"];
        if entry["type"] != "message" || message["role"] != "toolResult" {
            continue;
        }
        if message["isError"].as_bool() != Some(true) {
            continue;
        }
        failed.push(pi_one(message));
    }
    failed
}

// A Claude Code result that failed: its hook refusal, or what else it says
fn claude_one(text: &str, tool: Option<&str>) -> ToolError {
    match hook_refusal(text) {
        Some(command) if names_kelpies_check(command) => ToolError {
            kind: Kind::GuardBlock,
            hook: None,
        },
        Some(command) => ToolError {
            kind: Kind::HookBlock,
            hook: Some(command.to_owned()),
        },
        None => ToolError {
            kind: kind_of(text, tool),
            hook: None,
        },
    }
}

// A pi result that failed: its guard extension's refusal, or what else it says. The
// extension refuses a call that never ran by writing the refusal as the result's only
// block, and pi writes no `durationMs` for a call that never ran. It refuses a command
// that already ran by adding its reason as a block of its own at the end. A command's own
// output is one block, so output that starts with the mark or quotes it is not a refusal.
fn pi_one(message: &Value) -> ToolError {
    let texts: Vec<&str> = match &message["content"] {
        Value::String(text) => vec![text],
        content => blocks(content, "text")
            .filter_map(|block| block["text"].as_str())
            .collect(),
    };
    let marked = |text: &str| text.starts_with(GUARD_MARK);
    let ran = !message["durationMs"].is_null();
    let refused = (!ran && texts.len() == 1 && marked(texts[0]))
        || (texts.len() > 1 && texts.last().is_some_and(|last| marked(last)));
    ToolError {
        kind: match refused.then_some(Kind::GuardBlock) {
            Some(kind) => kind,
            None => kind_of(&texts.join("\n"), message["toolName"].as_str()),
        },
        hook: None,
    }
}

// What made a failed result fail, the first kind one of its lines says as its own error.
// A phrase quoted inside output is not an error line: each kind's phrase is read at the head
// of a line, once the markers a harness or a program put in front of its error are taken off.
fn kind_of(text: &str, tool: Option<&str>) -> Kind {
    let lines: Vec<Line> = text.lines().map(Line::of).collect();
    let head = |needle: &str| lines.iter().any(|line| line.head.starts_with(needle));
    // pi's own tool message is the whole line and names no file. A line that quotes it names
    // the file it quoted, so its phrase is read at the end only on a line that holds no colon.
    let tail = |needle: &str| {
        lines
            .iter()
            .any(|line| line.all.ends_with(needle) && !line.all.contains(':'))
    };
    for kind in IN_ORDER {
        let matched = match kind {
            Kind::ToolNotGiven => head("no such tool available"),
            Kind::UnknownSkill => head("unknown skill"),
            Kind::PermissionRefused => head("permission to use"),
            Kind::GitRefLock => head("cannot lock ref"),
            Kind::TmpRefused => lines
                .iter()
                .any(|line| line.head.starts_with("operation not permitted") && line.names_tmp()),
            Kind::PathRefused => lines
                .iter()
                .any(|line| line.head.starts_with("operation not permitted")),
            Kind::CommandNotFound => {
                head("no such file or directory")
                    || tail("is not available and could not be downloaded")
            }
            Kind::EditMiss => {
                head("string to replace not found in file")
                    || head("could not find edits")
                    || (head("found")
                        && lines
                            .iter()
                            .any(|line| line.holds("matches of the string to replace")))
            }
            Kind::Timeout => head("command timed out after"),
            Kind::Mcp => tool.is_some_and(|name| name.starts_with("mcp__")),
            Kind::Other => true,
            Kind::GuardBlock | Kind::HookBlock => false,
        };
        if matched {
            return kind;
        }
    }
    Kind::Other
}

// One line of a failed result: what it says, and what it says about itself. `head` is the line
// with the markers a harness or a program writes in front of its own error taken off: Claude
// Code's `<tool_use_error>` wrapper, and the `<something>:` run a program or a shell puts before
// its message. A line that quotes an error keeps its own words at its head, so no kind's phrase
// lands there.
#[derive(Debug)]
struct Line {
    head: String,
    all: String,
}

impl Line {
    fn of(raw: &str) -> Self {
        let mut line = raw.trim();
        if let Some(inner) = line.strip_prefix("<tool_use_error>") {
            line = inner
                .trim_end()
                .strip_suffix("</tool_use_error>")
                .unwrap_or(inner);
        }
        Self {
            head: message(line).to_lowercase(),
            all: raw.trim_end().to_lowercase(),
        }
    }

    // A sandbox or `confine` refusal names the path it refused
    fn names_tmp(&self) -> bool {
        self.all.contains("/tmp/")
    }

    fn holds(&self, needle: &str) -> bool {
        self.all.contains(needle)
    }
}

// Where a line's own message begins. A program or a shell writes its error after a run of
// `<token>:` markers, each followed by a space (`mkdir: cbm-docs: Operation not permitted`,
// `(eval):1: no such file or directory: /x`, `grep: /tmp/x: No such file or directory`), so the
// message is what follows the last such marker. A grep or rg match line writes no space after its
// colons (`notes.txt:3:Operation not permitted`), so it holds no marker and keeps its own words.
// A match line whose matched text has a colon and a space of its own is caught by naming the file
// and the line number it matched (`docs/x.md:7:Command timed out after 2m`).
fn message(line: &str) -> &str {
    if names_a_match(line) {
        return line;
    }
    let mut rest = line;
    let mut message = line;
    while let Some((token, after)) = rest.split_once(':') {
        if token.is_empty() || token.chars().any(char::is_whitespace) {
            break;
        }
        if after.starts_with(char::is_whitespace) {
            message = after.trim_start();
        }
        rest = after.trim_start();
    }
    message
}

// Whether a line begins by naming a file and a line in it, as grep and rg write a match
fn names_a_match(line: &str) -> bool {
    let Some((file, rest)) = line.split_once(':') else {
        return false;
    };
    let number = rest.split(':').next().unwrap_or_default();
    (file.contains('/') || file.contains('.'))
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

// `PreToolUse:Bash hook error: [<the hook's command>]: <its reason>`, as the whole first line.
// The command is what sits between the brackets, cut at the first `]: ` since a command can hold
// a `]` of its own (`bash -c '[ -f .gate ] && exit 2'`).
fn hook_refusal(text: &str) -> Option<&str> {
    let first = text.lines().next()?;
    let (head, rest) = first.split_once(" hook error: [")?;
    let named = |word: &str| {
        let mut chars = word.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let words: Vec<&str> = head.split(':').collect();
    if words.len() > 2 || !words.iter().all(|w| named(w)) {
        return None;
    }
    let end = rest.find("]: ").or_else(|| rest.find(']'))?;
    Some(&rest[..end])
}

// Whether a hook's command is kelpie's own check. The settings write kelpie's binary first
// and its `guard` or `confine` program second, so a command that merely names `guard` for
// some other program is the project's own hook.
fn names_kelpies_check(command: &str) -> bool {
    let mut words = shell_words(command).into_iter();
    let program = words.next().unwrap_or_default();
    (program.rsplit('/').next().unwrap_or_default()) == KELPIE_PROGRAM
        && words
            .next()
            .is_some_and(|sub| sub == "guard" || sub == "confine")
}

// A shell command's words, each quoted part kept as one word. kelpie's own hook commands are
// shell-quoted, so a binary under a folder with a space in its name is one word here and
// several under a plain split.
fn shell_words(command: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                open = true;
            }
            None if c.is_whitespace() => {
                if open {
                    words.push(std::mem::take(&mut word));
                }
                open = false;
            }
            None => {
                word.push(c);
                open = true;
            }
        }
    }
    if open {
        words.push(word);
    }
    words
}

// The transcript's lines from byte `from` on, as JSON. A file shorter than `from` is a new one.
fn entries(text: &str, from: usize) -> impl Iterator<Item = Value> {
    (text.get(from..).unwrap_or(text))
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
}

// A message's blocks of type `kind`
fn blocks<'a>(content: &'a Value, kind: &str) -> impl Iterator<Item = &'a Value> {
    (content.as_array().into_iter().flatten()).filter(move |block| block["type"] == kind)
}

// A tool result's text, which one harness writes as a string and the other as blocks
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
