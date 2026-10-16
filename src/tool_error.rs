//! What a call's transcript says of each tool call that failed: what made it
//! fail, as a kind
//!
//! A failed result counts once, however many lines of output it holds: one
//! command can print the same error on hundreds of lines. A kind is the first
//! that matches, so a guard refusal of a result that also holds a sandbox
//! error is a guard refusal. Claude Code names the hook that refused inside its
//! refusal text; pi's guard extension marks its refusals with [`GUARD_MARK`],
//! which nothing else writes. `docs/facts/tool-errors.md`

use std::collections::HashMap;

use serde_json::Value;

/// What a pi call's guard extension puts at the front of every refusal it makes
pub(crate) const GUARD_MARK: &str = "kelpie guard: ";

/// Why a tool call failed, as a series' `kind` label
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
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
    pub(crate) fn as_str(self) -> &'static str {
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
pub(crate) struct ToolError {
    /// What made it fail
    pub(crate) kind: Kind,
    /// The hook that refused it, which Claude Code names in its refusal, and which
    /// a log line carries since no label can hold it
    pub(crate) hook: Option<String>,
}

/// The tool calls Claude Code's transcript `text` says failed, from byte `from`
/// on, where the call whose lines begin there made them
pub(crate) fn claude(text: &str, from: usize) -> Vec<ToolError> {
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
pub(crate) fn pi(text: &str, from: usize) -> Vec<ToolError> {
    let mut failed = Vec::new();
    for entry in entries(text, from) {
        let message = &entry["message"];
        if entry["type"] != "message" || message["role"] != "toolResult" {
            continue;
        }
        if message["isError"].as_bool() != Some(true) {
            continue;
        }
        let tool = message["toolName"].as_str();
        failed.push(pi_one(&text_of(&message["content"]), tool));
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

// A pi result that failed: its guard extension's refusal, or what else it says. A refusal of a
// call is the whole result; a refusal of a command that already ran is feedback joined to its end.
fn pi_one(text: &str, tool: Option<&str>) -> ToolError {
    let marked = |line: &str| line.starts_with(GUARD_MARK);
    let refused = text.lines().next().is_some_and(marked)
        || (text.lines().rev())
            .find(|line| !line.trim().is_empty())
            .is_some_and(marked);
    ToolError {
        kind: match refused.then_some(Kind::GuardBlock) {
            Some(kind) => kind,
            None => kind_of(text, tool),
        },
        hook: None,
    }
}

// What made a failed result fail, the first kind one of its lines says. Output that
// quotes an error is told from the error itself by matching a line, not a phrase.
fn kind_of(text: &str, tool: Option<&str>) -> Kind {
    let lines: Vec<String> = text.lines().map(str::to_lowercase).collect();
    let holds = |needle: &str| lines.iter().any(|line| line.contains(needle));
    let denied = |line: &str| line.contains("operation not permitted");
    for kind in IN_ORDER {
        let matched = match kind {
            Kind::ToolNotGiven => holds("no such tool available"),
            Kind::UnknownSkill => holds("unknown skill"),
            Kind::PermissionRefused => holds("has been denied"),
            Kind::GitRefLock => lines
                .iter()
                .any(|line| line.starts_with("error: cannot lock ref")),
            Kind::TmpRefused => lines
                .iter()
                .any(|line| denied(line) && line.contains("/tmp/")),
            Kind::PathRefused => lines.iter().any(|line| denied(line)),
            Kind::CommandNotFound => {
                holds("no such file or directory")
                    || holds("is not available and could not be downloaded")
            }
            Kind::EditMiss => {
                holds("string to replace not found in file")
                    || holds("matches of the string to replace")
                    || holds("oldtext must match exactly")
            }
            Kind::Timeout => holds("timed out after"),
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

// `PreToolUse:Bash hook error: [<the hook's command>]: <its reason>`, as the whole first line
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
    let (command, _) = rest.split_once(']')?;
    Some(command)
}

// Whether a hook's command runs kelpie's own `guard` or `confine`
fn names_kelpies_check(command: &str) -> bool {
    command
        .split_whitespace()
        .map(|word| word.trim_matches(['\'', '"']))
        .any(|word| word == "guard" || word == "confine")
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
