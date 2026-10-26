//! What a call's transcript says once it ends, as series and a standing
//! warning: the MCP servers it was given and its tool calls to them, each
//! request it made to its model, and each of its tool calls that failed
//!
//! A server that is not connected as the call ends is noted once, naming the
//! call and why: Claude Code's error code, or that pi's session left none of
//! the server's tools in it as it ended. The note stands until a later call
//! finds the server connected. A call that wrote nothing to its transcript
//! reached no model, and counts nothing. A Codex rollout is read for the
//! requests alone: its MCP and tool calls wait on #1279.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::Runner;
use super::claim::Notes;
use super::code_graph::{who, who_carried};
use crate::mcp_use::{self, McpUse, Stood};
use crate::metrics::{OWN_SERVERS, Recorder};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, Role, SessionId, Transcript,
};
use crate::request_use::{self, Request};
use crate::settings::Harness;
use crate::ticket::TicketId;
use crate::tool_error::{self, ToolError};
use crate::transcripts;

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
    Codex,
}

impl Format {
    // pi names no server that failed to connect: what its session leaves
    // available and what it answers say which connected, and its tool calls are
    // counted as Claude Code's are. Codex's rollout is read for its requests alone, and for a
    // worker's calls alone, to keep a Codex reviewer's and project manager's series off the dog's cap.
    fn of(call: &AgentCall) -> Option<Self> {
        let harness = call.harness.harness();
        match Self::of_harness(harness) {
            Some(Self::Codex) if call.role != Role::Worker => None,
            format => format,
        }
    }

    fn of_harness(harness: Harness) -> Option<Self> {
        // parity: harness.mcp-tool-calls, harness.mcp-failures, harness.request-usage,
        // harness.tool-errors
        let facts = harness.facts();
        match facts.transcript.filter(|_| facts.counts_transcript) {
            Some(transcripts::Format::Claude) => Some(Self::Claude),
            Some(transcripts::Format::Pi) => Some(Self::Pi),
            Some(transcripts::Format::Codex) => Some(Self::Codex),
            _ => None,
        }
    }

    fn transcripts(self) -> transcripts::Format {
        match self {
            Self::Claude => transcripts::Format::Claude,
            Self::Pi => transcripts::Format::Pi,
            Self::Codex => transcripts::Format::Codex,
        }
    }

    // Whether the format records the MCP servers and tool calls of a call
    fn records_calls(self) -> bool {
        !matches!(self, Self::Codex)
    }

    fn read(self, text: &str, from: usize) -> McpUse {
        match self {
            Self::Claude => mcp_use::claude(text, from),
            Self::Pi => mcp_use::pi(text, from),
            Self::Codex => McpUse::default(),
        }
    }

    fn requests(self, text: &str, from: usize) -> Vec<Request> {
        match self {
            Self::Claude => request_use::claude(text, from),
            Self::Pi => request_use::pi(text, from),
            Self::Codex => request_use::codex(text, from),
        }
    }

    fn errors(self, text: &str, from: usize) -> Vec<ToolError> {
        match self {
            Self::Claude => tool_error::claude(text, from),
            Self::Pi => tool_error::pi(text, from),
            Self::Codex => Vec::new(),
        }
    }

    // The part of a tool's name the harness gives the server `name`
    fn tool_part(self, name: &str) -> String {
        match self {
            Self::Claude => mcp_use::claude_tool_part(name),
            Self::Pi | Self::Codex => crate::ports::mcp::tool_prefix(name),
        }
    }
}

// What a server's transcript says about it, where it names no reason: pi says which of its servers
// it reached, and nothing of one it did not
const NO_TOOLS: &str = "no-tools";

// How each server the call was given stood, as its transcript says: Claude Code names those that
// failed, pi those it reached, whose tools its session leaves available as it ends or whose tool
// calls it answered. A server with neither failed, and a session that lists no tools and answers no
// call says its servers were none of them there.
fn stood(format: Format, given: &[String], used: &McpUse) -> Option<Stood> {
    if let Some(stood) = &used.servers {
        return Some((*stood).clone());
    }
    let reached = used.reached.as_ref()?;
    Some(Stood {
        failed: given
            .iter()
            .filter(|name| !reached.contains(&format.tool_part(name.as_str())))
            .map(|name| (name.clone(), NO_TOOLS.to_owned()))
            .collect(),
        pending: Vec::new(),
    })
}

// The file the session `transcript` names is written in, once it is: found as the harness names
// it, from `at`, when the call began
fn file(
    transcript: &Transcript,
    session: &str,
    format: Format,
    at: jiff::Timestamp,
) -> Option<PathBuf> {
    let threads = transcript.threads.as_deref();
    transcripts::find(
        format.transcripts(),
        session,
        &transcript.folder,
        threads,
        at,
    )
}

fn length(file: &Path) -> u64 {
    std::fs::metadata(file).map_or(0, |m| m.len())
}

// Count each failed tool call of `role`, and name each hook that is not kelpie's in a note,
// since no label may hold a command. The wrapper does this for a turn it began, and a carried
// turn does it for the one its own image began.
fn counted_errors(who: &str, role: &str, errors: &[ToolError], metrics: &Recorder, notes: &Notes) {
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    for error in errors {
        metrics.tool_error(role, error.kind.as_str());
        if let Some(hook) = &error.hook {
            *refused.entry(hook.clone()).or_insert(0) += 1;
        }
    }
    for (hook, count) in refused {
        let calls = match count {
            1 => "one tool call".to_owned(),
            n => format!("{n} tool calls"),
        };
        notes.push(format!("{who} had {calls} refused by the hook `{hook}`"));
    }
}

/// Where a worker's turn found its session's transcript as it began, which a handover carries so
/// the next image counts the turn's requests when it ends
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Mark {
    folder: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    threads: Option<PathBuf>,
    session: String,
    from: u64,
    // When the turn began, in seconds since the epoch
    #[serde(default)]
    at: i64,
}

impl Mark {
    /// Where `call`'s transcript stands now, for a call on a harness whose requests are counted
    pub(super) fn of(agents: &dyn Agents, call: &AgentCall) -> Option<Self> {
        let format = Format::of(call)?;
        let transcript = agents.transcript(call)?;
        let session = call.session.id().0.clone();
        let at = jiff::Timestamp::now();
        let from = file(&transcript, &session, format, at).map_or(0, |file| length(&file));
        Some(Self {
            folder: transcript.folder,
            threads: transcript.threads,
            session,
            from,
            at: at.as_second(),
        })
    }

    /// Counts the requests and the failed tool calls the turn made since, as the
    /// wrapper does for a turn this image began
    pub(super) fn count(
        &self,
        harness: Harness,
        ticket: TicketId,
        metrics: &Recorder,
        notes: &Notes,
    ) {
        let Some(format) = Format::of_harness(harness) else {
            return;
        };
        let transcript = Transcript {
            folder: self.folder.clone(),
            threads: self.threads.clone(),
        };
        let at = jiff::Timestamp::from_second(self.at).unwrap_or_default();
        let Some(file) = file(&transcript, &self.session, format, at) else {
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
            if format.records_calls() {
                counted_errors(
                    &who_carried(ticket),
                    Role::Worker.as_str(),
                    &format.errors(&text, from),
                    metrics,
                    notes,
                );
            }
        }
    }
}

impl TranscriptCount {
    fn file(&self, call: &AgentCall, format: Format, at: jiff::Timestamp) -> Option<PathBuf> {
        let transcript = self.agents.transcript(call)?;
        file(&transcript, &call.session.id().0, format, at)
    }

    fn count_requests(&self, call: &AgentCall, format: Format, text: &str, from: usize) {
        let (role, harness) = (call.role.as_str(), call.harness.harness().as_str());
        for request in format.requests(text, from) {
            self.metrics.request(role, harness, &request);
        }
    }

    // Each failed tool call of the roles whose requests are counted
    fn count_errors(&self, call: &AgentCall, format: Format, text: &str, from: usize) {
        counted_errors(
            &who(call),
            call.role.as_str(),
            &format.errors(text, from),
            &self.metrics,
            &self.notes,
        );
    }

    fn count_mcp(&self, call: &AgentCall, format: Format, used: &McpUse) {
        let given: Vec<String> = crate::ports::mcp::servers(call).keys().cloned().collect();
        for name in &given {
            self.metrics.mcp_tool_calls(name, 0);
        }
        for (part, calls) in &used.tool_calls {
            let named = (given.iter().map(String::as_str))
                .chain(OWN_SERVERS)
                .find(|name| format.tool_part(name) == *part);
            self.metrics.mcp_tool_calls(named.unwrap_or(part), *calls);
        }
        let Some(stood) = stood(format, &given, used) else {
            return;
        };
        for name in given.iter().filter(|name| !stood.pending.contains(name)) {
            let key = format!("mcp-server-failed-{name}");
            match stood.failed.get(name) {
                Some(code) => {
                    self.metrics.mcp_server(name, false);
                    let why = match code.as_str() {
                        NO_TOOLS => {
                            "its tools were not in the session as the call ended".to_owned()
                        }
                        other => format!("which failed to connect: {other}"),
                    };
                    self.notes.standing(
                        &key,
                        format!("{} ran without the MCP server `{name}`, {why}", who(call)),
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
        let at = jiff::Timestamp::now();
        let before = self.file(call, format, at).as_deref().map_or(0, length);
        let ran = self.agents.run(call, ending);
        let text =
            (self.file(call, format, at)).and_then(|file| std::fs::read_to_string(file).ok());
        if let Some(text) = text.filter(|text| text.len() as u64 > before) {
            let from = usize::try_from(before).unwrap_or(0);
            if format.records_calls() && crate::ports::mcp::loads(call) {
                self.count_mcp(call, format, &format.read(&text, from));
            }
            self.count_requests(call, format, &text, from);
            if format.records_calls() {
                self.count_errors(call, format, &text, from);
            }
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
