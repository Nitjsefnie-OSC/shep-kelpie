//! What a call's transcript says once it ends, as series and a standing
//! warning: the MCP servers it was given and its tool calls to them, each
//! request it made to its model, and each of its tool calls that failed
//!
//! A server that failed to connect is noted once, naming the call and its
//! error code, and the note stands until a later call finds it connected. A
//! call that wrote nothing to its transcript reached no model, and counts
//! nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::Runner;
use super::claim::Notes;
use super::code_graph::who;
use crate::mcp_use::{self, McpUse};
use crate::metrics::{OWN_SERVERS, Recorder};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, Role, SessionId, Transcript,
};
use crate::request_use::{self, Request};
use crate::settings::{AgentHarness, Harness};
use crate::tool_error::{self, ToolError};

/// Agents that count each call's MCP servers, tool calls and requests as it ends
pub(super) struct TranscriptCount {
    agents: Arc<dyn Agents>,
    notes: Notes,
    metrics: Arc<Recorder>,
}

impl Runner {
    /// `agents`, counting the MCP servers, tool calls and requests of each call they run
    pub(super) fn counted(&self, agents: Arc<dyn Agents>) -> Arc<dyn Agents> {
        Arc::new(TranscriptCount {
            agents,
            notes: self.notes.clone(),
            metrics: Arc::clone(&self.metrics),
        })
    }
}

// How a harness writes its transcript
#[derive(Debug, Clone, Copy)]
enum Format {
    Claude,
    Pi,
}

impl Format {
    // pi records no server that failed to connect, so only its tool calls are counted.
    fn of(call: &AgentCall) -> Option<Self> {
        // parity: harness.mcp-tool-calls, harness.mcp-failures, harness.request-usage,
        // harness.tool-errors
        match call.harness {
            AgentHarness::ClaudeCode => Some(Self::Claude),
            AgentHarness::Pi(_) => Some(Self::Pi),
            _ => None,
        }
    }

    fn of_harness(harness: Harness) -> Option<Self> {
        // parity: harness.request-usage, harness.tool-errors
        match harness {
            Harness::ClaudeCode => Some(Self::Claude),
            Harness::Pi => Some(Self::Pi),
            _ => None,
        }
    }

    fn read(self, text: &str, from: usize) -> McpUse {
        match self {
            Self::Claude => mcp_use::claude(text, from),
            Self::Pi => mcp_use::pi(text, from),
        }
    }

    fn requests(self, text: &str, from: usize) -> Vec<Request> {
        match self {
            Self::Claude => request_use::claude(text, from),
            Self::Pi => request_use::pi(text, from),
        }
    }

    fn errors(self, text: &str, from: usize) -> Vec<ToolError> {
        match self {
            Self::Claude => tool_error::claude(text, from),
            Self::Pi => tool_error::pi(text, from),
        }
    }

    // The part of a tool's name the harness gives the server `name`
    fn tool_part(self, name: &str) -> String {
        match self {
            Self::Claude => mcp_use::claude_tool_part(name),
            Self::Pi => crate::adapters::pi_tool_prefix(name),
        }
    }
}

// The file a session writes in `folder`, once it is written: Claude Code names it for the session,
// and pi `<time>_<session>.jsonl`.
fn file(folder: &Path, session: &str, format: Format) -> Option<PathBuf> {
    match format {
        Format::Claude => Some(folder.join(format!("{session}.jsonl"))),
        Format::Pi => {
            let suffix = format!("_{session}.jsonl");
            (std::fs::read_dir(folder).ok()?)
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    (path.file_name().and_then(|n| n.to_str()))
                        .is_some_and(|n| n.ends_with(&suffix))
                })
        }
    }
}

fn length(file: &Path) -> u64 {
    std::fs::metadata(file).map_or(0, |m| m.len())
}

/// Where a worker's turn found its session's transcript as it began, which a handover carries so
/// the next image counts the turn's requests when it ends
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Mark {
    folder: PathBuf,
    session: String,
    from: u64,
}

impl Mark {
    /// Where `call`'s transcript stands now, for a call on a harness whose requests are counted
    pub(super) fn of(agents: &dyn Agents, call: &AgentCall) -> Option<Self> {
        let format = Format::of(call)?;
        let folder = agents.transcript(call)?.folder;
        let session = call.session.id().0.clone();
        let from = file(&folder, &session, format).map_or(0, |file| length(&file));
        Some(Self {
            folder,
            session,
            from,
        })
    }

    /// Counts the requests and the failed tool calls the turn made since, as the
    /// wrapper does for a turn this image began
    pub(super) fn count(&self, harness: Harness, metrics: &Recorder) {
        let Some(format) = Format::of_harness(harness) else {
            return;
        };
        let Some(file) = file(&self.folder, &self.session, format) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            return;
        };
        if text.len() as u64 > self.from {
            let from = usize::try_from(self.from).unwrap_or(0);
            for request in format.requests(&text, from) {
                metrics.request(Role::Worker.as_str(), harness.as_str(), &request);
            }
            for error in format.errors(&text, from) {
                metrics.tool_error(Role::Worker.as_str(), error.kind.as_str());
            }
        }
    }
}

impl TranscriptCount {
    fn file(&self, call: &AgentCall, format: Format) -> Option<PathBuf> {
        let folder = self.agents.transcript(call)?.folder;
        file(&folder, &call.session.id().0, format)
    }

    fn count_requests(&self, call: &AgentCall, format: Format, text: &str, from: usize) {
        let (role, harness) = (call.role.as_str(), call.harness.harness().as_str());
        for request in format.requests(text, from) {
            self.metrics.request(role, harness, &request);
        }
    }

    // Each failed tool call of the roles whose requests are counted. A hook that is not
    // kelpie's is named in a note, since no label may hold a command.
    fn count_errors(&self, call: &AgentCall, format: Format, text: &str, from: usize) {
        let role = call.role.as_str();
        let mut refused: BTreeMap<String, u64> = BTreeMap::new();
        for error in format.errors(text, from) {
            self.metrics.tool_error(role, error.kind.as_str());
            if let Some(hook) = &error.hook {
                *refused.entry(hook.clone()).or_insert(0) += 1;
            }
        }
        for (hook, count) in refused {
            let calls = match count {
                1 => "one tool call".to_owned(),
                n => format!("{n} tool calls"),
            };
            self.notes
                .push(format!("{} had {calls} refused by the hook `{hook}`", who(call)));
        }
    }

    fn count_mcp(&self, call: &AgentCall, format: Format, used: &McpUse) {
        let given: Vec<String> = crate::adapters::mcp::servers(call)
            .keys()
            .cloned()
            .collect();
        for name in &given {
            self.metrics.mcp_tool_calls(name, 0);
        }
        for (part, calls) in &used.tool_calls {
            let named = (given.iter().map(String::as_str))
                .chain(OWN_SERVERS)
                .find(|name| format.tool_part(name) == *part);
            self.metrics.mcp_tool_calls(named.unwrap_or(part), *calls);
        }
        let Some(stood) = &used.servers else {
            return;
        };
        for name in given.iter().filter(|name| !stood.pending.contains(name)) {
            let key = format!("mcp-server-failed-{name}");
            match stood.failed.get(name) {
                Some(code) => {
                    self.metrics.mcp_server(name, false);
                    self.notes.standing(
                        &key,
                        format!(
                            "{} ran without the MCP server `{name}`, which failed to connect: \
                             {code}",
                            who(call)
                        ),
                    );
                }
                None => {
                    self.metrics.mcp_server(name, true);
                    self.notes.ends(&key);
                }
            }
        }
    }
}

impl std::fmt::Debug for TranscriptCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptCount").finish_non_exhaustive()
    }
}

impl Agents for TranscriptCount {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.agents.prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let Some(format) = Format::of(call) else {
            return self.agents.run(call, ending);
        };
        let before = self.file(call, format).as_deref().map_or(0, length);
        let ran = self.agents.run(call, ending);
        let text = (self.file(call, format)).and_then(|file| std::fs::read_to_string(file).ok());
        if let Some(text) = text.filter(|text| text.len() as u64 > before) {
            let from = usize::try_from(before).unwrap_or(0);
            if crate::adapters::mcp::loads(call) {
                self.count_mcp(call, format, &format.read(&text, from));
            }
            self.count_requests(call, format, &text, from);
            self.count_errors(call, format, &text, from);
        }
        ran
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        self.agents.transcript(call)
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        self.agents.last_active(call)
    }

    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        self.agents.foreground(call)
    }

    fn foreground_ended(&self, session: &SessionId) {
        self.agents.foreground_ended(session);
    }
}

#[cfg(test)]
mod tests;
