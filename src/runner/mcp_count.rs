//! The MCP servers a call was given and its tool calls to them, read from its
//! transcript once it ends, as series and a standing warning
//!
//! A server that failed to connect is noted once, naming the call and its
//! error code, and the note stands until a later call finds it connected. A
//! call that wrote nothing to its transcript reached no model, and counts
//! nothing.

use std::path::PathBuf;
use std::sync::Arc;

use super::Runner;
use super::claim::Notes;
use super::code_graph::who;
use crate::mcp_use::{self, McpUse};
use crate::metrics::{OWN_SERVERS, Recorder};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, SessionId, Transcript,
};
use crate::settings::AgentHarness;

/// Agents that count each call's MCP servers and tool calls as it ends
pub(super) struct McpCount {
    agents: Arc<dyn Agents>,
    notes: Notes,
    metrics: Arc<Recorder>,
}

impl Runner {
    /// `agents`, counting the MCP servers and tool calls of each call they run
    pub(super) fn counted(&self, agents: Arc<dyn Agents>) -> Arc<dyn Agents> {
        Arc::new(McpCount {
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
        // parity: harness.mcp-tool-calls, harness.mcp-failures
        match call.harness {
            AgentHarness::ClaudeCode => Some(Self::Claude),
            AgentHarness::Pi(_) => Some(Self::Pi),
            _ => None,
        }
    }

    fn read(self, text: &str, from: usize) -> McpUse {
        match self {
            Self::Claude => mcp_use::claude(text, from),
            Self::Pi => mcp_use::pi(text, from),
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

impl McpCount {
    // The file `call`'s session writes, once it is written: Claude Code names it for the session,
    // and pi `<time>_<session>.jsonl`.
    fn file(&self, call: &AgentCall, format: Format) -> Option<PathBuf> {
        let folder = self.agents.transcript(call)?.folder;
        let session = &call.session.id().0;
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

    fn count(&self, call: &AgentCall, format: Format, used: &McpUse) {
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

impl std::fmt::Debug for McpCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpCount").finish_non_exhaustive()
    }
}

impl Agents for McpCount {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.agents.prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let format = Format::of(call).filter(|_| crate::adapters::mcp::loads(call));
        let Some(format) = format else {
            return self.agents.run(call, ending);
        };
        let length = |file: &PathBuf| std::fs::metadata(file).map_or(0, |m| m.len());
        let before = self.file(call, format).as_ref().map_or(0, length);
        let ran = self.agents.run(call, ending);
        let text = (self.file(call, format)).and_then(|file| std::fs::read_to_string(file).ok());
        if let Some(text) = text.filter(|text| text.len() as u64 > before) {
            let from = usize::try_from(before).unwrap_or(0);
            self.count(call, format, &format.read(&text, from));
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
