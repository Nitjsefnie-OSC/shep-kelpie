//! What a call's transcript says of its MCP servers: those not connected as
//! it ended, and its tool calls to each
//!
//! Claude Code writes how a session's servers stand in an `attachment` line,
//! and another only when that changes, so the last such line in the whole
//! transcript is how they stood at the call's end. While every server connects
//! and none of their tools is deferred it writes none, and its prompt snapshot
//! lists each server's tools. pi's session file records tool calls and the
//! tools its system messages add and remove, which is all it says of a server
//! that failed to connect. `docs/facts/mcp-servers.md`

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Why a server that needs a sign-in is not connected
pub const NEEDS_AUTH: &str = "NEEDS_AUTH";

/// What a call's transcript says of its MCP servers
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct McpUse {
    /// How its servers stood as the call ended, where the transcript says.
    /// Claude Code says this, and pi does not
    pub servers: Option<Stood>,
    /// The servers the session shows working as it ends, by the part of a
    /// tool's name each is named after: those whose tools its system messages
    /// leave available, and those whose tool calls it answered. pi says this,
    /// and names no server that failed
    pub reached: Option<BTreeSet<String>>,
    /// The call's tool calls to MCP servers, by the server's part of the
    /// tool's name, `mcp__<server>__<tool>`
    pub tool_calls: BTreeMap<String, u64>,
}

/// How a session's MCP servers stood
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stood {
    /// The servers not connected, each with why: Claude Code's error code, or
    /// [`NEEDS_AUTH`]
    pub failed: BTreeMap<String, String>,
    /// The servers still connecting
    pub pending: Vec<String>,
}

/// What Claude Code's transcript `text` says, of the call whose lines begin
/// at byte `from`
pub fn claude(text: &str, from: usize) -> McpUse {
    let servers = (text.lines().rev())
        .filter(|line| line.contains("McpServers"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| stood(&entry["attachment"]))
        .or_else(|| all_connected(text));
    let tool_calls = tool_calls(own_lines(text, from), |entry| {
        (entry["type"] == "assistant").then(|| tools(&entry["message"]["content"], "tool_use"))
    });
    McpUse {
        servers,
        reached: None,
        tool_calls,
    }
}

/// What pi's session file `text` says, of the call whose lines begin at byte
/// `from`
pub fn pi(text: &str, from: usize) -> McpUse {
    let tool_calls = tool_calls(own_lines(text, from), |entry| {
        let message = &entry["message"];
        (entry["type"] == "message" && message["role"] == "assistant")
            .then(|| tools(&message["content"], "toolCall"))
    });
    McpUse {
        servers: None,
        reached: Some(reached(text)),
        tool_calls,
    }
}

/// The part of a tool's name Claude Code gives its server `name`: every
/// character but letters, digits, `_` and `-` as `_`
pub fn claude_tool_part(name: &str) -> String {
    let safe = |c: char| match c.is_ascii_alphanumeric() || c == '_' || c == '-' {
        true => c,
        false => '_',
    };
    name.chars().map(safe).collect()
}

// The lines from byte `from` on, or all of them where the file is shorter: a new file.
fn own_lines(text: &str, from: usize) -> impl Iterator<Item = &str> {
    text.get(from..).unwrap_or(text).lines()
}

// How an attachment says the servers stood, if it says
fn stood(attachment: &Value) -> Option<Stood> {
    let failed = attachment["failedMcpServers"].as_array()?;
    let names = |key: &str| -> Vec<String> {
        (attachment[key].as_array().into_iter().flatten())
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect()
    };
    let mut stood = Stood {
        failed: (failed.iter())
            .filter_map(|server| {
                let name = server["name"].as_str()?;
                let code = server["errorCode"].as_str().unwrap_or("unknown");
                Some((name.to_owned(), code.to_owned()))
            })
            .collect(),
        pending: names("pendingMcpServers"),
    };
    for name in names("needsAuthMcpServers") {
        stood.failed.insert(name, NEEDS_AUTH.to_owned());
    }
    Some(stood)
}

// The servers all connected, where the last prompt snapshot that lists tools
// lists one of a server: a server that failed would have its own line
fn all_connected(text: &str) -> Option<Stood> {
    (text.lines().rev())
        .filter(|line| line.contains("prompt_snapshot"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| {
            let tools = entry["attachment"]["tools"].as_array()?;
            Some(tools.iter().any(|tool| tool["server"].is_string()))
        })
        .and_then(|served| served.then(Stood::default))
}

// The servers the session shows working as it ends, by the part of a tool's name pi names them
// after. The file's last word on a server is how it stood: an addition, or a call answered with
// `isError` false, leaves it standing, and a removal of its last tool does not. `isError` true
// says only that the tool was called. The whole file is read, as a resumed call's holds earlier adds.
fn reached(text: &str) -> BTreeSet<String> {
    let mut tools: BTreeSet<String> = BTreeSet::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let entries = (text.lines()).filter_map(|line| serde_json::from_str::<Value>(line).ok());
    for entry in entries {
        let message = &entry["message"];
        match message["role"].as_str() {
            Some("system") => {
                for (key, added) in [("toolsAdded", true), ("toolsRemoved", false)] {
                    let names = (message[key].as_array().into_iter().flatten())
                        .filter_map(|tool| (tool["name"].as_str()).map(str::to_owned));
                    for name in names {
                        let Some(server) = server_part(&name).map(str::to_owned) else {
                            continue;
                        };
                        if added {
                            tools.insert(name);
                        } else {
                            tools.remove(&name);
                        }
                        let left = (tools.iter().filter_map(|tool| server_part(tool)))
                            .any(|part| part == server);
                        if left {
                            reached.insert(server);
                        } else {
                            reached.remove(&server);
                        }
                    }
                }
            }
            Some("toolResult") => {
                if message["isError"].as_bool().unwrap_or(false) {
                    continue;
                }
                let server = (message["toolName"].as_str()).and_then(server_part);
                if let Some(server) = server {
                    reached.insert(server.to_owned());
                }
            }
            _ => {}
        }
    }
    reached
}

// The server part of a tool's name `mcp__<server>__<tool>`, if it has one
fn server_part(tool: &str) -> Option<&str> {
    (tool.strip_prefix("mcp__"))
        .and_then(|rest| rest.split_once("__"))
        .map(|(server, _)| server)
}

// The names of a message's tool calls, blocks of type `kind`
fn tools(content: &Value, kind: &str) -> Vec<String> {
    (content.as_array().into_iter().flatten())
        .filter(|block| block["type"] == kind)
        .filter_map(|block| block["name"].as_str().map(str::to_owned))
        .collect()
}

// The MCP tool calls of `lines`, by server part, as `calls` reads each line's
fn tool_calls<'a>(
    lines: impl Iterator<Item = &'a str>,
    calls: impl Fn(&Value) -> Option<Vec<String>>,
) -> BTreeMap<String, u64> {
    let mut counted = BTreeMap::new();
    let entries = lines.filter_map(|line| serde_json::from_str::<Value>(line).ok());
    for tool in entries.filter_map(|entry| calls(&entry)).flatten() {
        if let Some(server) = server_part(&tool) {
            *counted.entry(server.to_owned()).or_insert(0) += 1;
        }
    }
    counted
}

#[cfg(test)]
mod tests;
