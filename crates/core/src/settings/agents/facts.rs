//! What is true of each harness, in one table
//!
//! [`Harness::facts`] is the only place that tells the harnesses apart by name.
//! Everything else that varies by harness reads a fact from it, so a new CLI
//! is one more row here and no new `match` elsewhere.

use super::{AgentHarness, Harness, ModelServer, UsageReader};
use crate::transcripts::Format;

/// Who gives a session its id
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIds {
    /// Kelpie names the session when it starts it
    Kelpie,
    /// The CLI names it, so kelpie keeps which of its ids each one is
    Cli,
}

/// What is true of one harness
#[derive(Debug, Clone, Copy)]
pub struct HarnessFacts {
    /// The name settings and agent files write
    pub name: &'static str,
    /// The program that runs it, as an error names it
    pub program: &'static str,
    /// The one way its usage is read, or `None` for a stand-in that takes any
    pub usage: Option<UsageReader>,
    /// Whether it runs a model on a server that kelpie names as `url` or
    /// `gateway`, with a `context` size, and holds a forwarder for its calls
    pub needs_model_server: bool,
    /// Whether an agent on it can name `tools`, an allow-list
    pub takes_tools: bool,
    /// Whether it runs `worker.guard_hooks`, which are Claude Code hooks
    pub runs_guard_hooks: bool,
    /// Why it cannot run an `after_command` hook, or `None` when it can
    pub no_after_command: Option<&'static str>,
    /// Who gives a session its id
    pub sessions: SessionIds,
    /// The tool a reviewer on it writes its test with, or `None` when the
    /// fence cannot hold its writes to a folder
    pub writing_tool: Option<&'static str>,
    /// Whether a call on it loads the `.claude` folder of the folder it runs in
    pub reads_claude_files: bool,
    /// Whether a pause ends a turn on it at its next tool result
    pub ends_at_tool: bool,
    /// Whether a worker's turn on it can load the browser's MCP bridge
    pub drives_a_browser: bool,
    /// Whether a session keeps the MCP servers it began with
    pub keeps_its_servers: bool,
    /// Whether a reviewer on it that only reads gets an empty folder of its own
    pub reads_from_an_empty_folder: bool,
    /// The format of the transcript it writes, if kelpie reads it
    pub transcript: Option<Format>,
    /// Whether the runner counts a turn's MCP and tool calls from that transcript
    pub counts_calls: bool,
    /// Whether it can run the roles that need Claude Code's own tools: the
    /// issue writer and the project manager
    pub runs_claude_roles: bool,
    /// Why a handover waits while a session on it is open in the maintainer's
    /// terminal, if it does
    pub holds_handover_at_attach: Option<&'static str>,
}

impl Harness {
    /// Every harness a project can name
    pub const ALL: [Self; 4] = [Self::ClaudeCode, Self::Pi, Self::Codex, Self::Goose];

    /// What is true of it
    pub fn facts(self) -> &'static HarnessFacts {
        match self {
            Self::ClaudeCode => &CLAUDE_CODE,
            Self::Pi => &PI,
            Self::Codex => &CODEX,
            Self::Goose => &GOOSE,
            #[cfg(any(test, feature = "test-support"))]
            Self::StandIn => &STAND_IN,
        }
    }
}

impl AgentHarness {
    /// `harness` with the `server` it runs its model on, if it needs one
    ///
    /// `None` when `harness` needs a server and `server` is `None`, or the
    /// reverse. A stand-in takes either.
    pub fn on(harness: Harness, server: Option<ModelServer>) -> Option<Self> {
        match (harness, server) {
            (Harness::ClaudeCode, None) => Some(Self::ClaudeCode),
            (Harness::Pi, Some(server)) => Some(Self::Pi(server)),
            (Harness::Codex, None) => Some(Self::Codex),
            (Harness::Goose, Some(server)) => Some(Self::Goose(server)),
            #[cfg(any(test, feature = "test-support"))]
            (Harness::StandIn, _) => Some(Self::StandIn),
            _ => None,
        }
    }

    /// The server its model runs on, for a harness that needs one
    pub fn server(&self) -> Option<&ModelServer> {
        match self {
            Self::Pi(server) | Self::Goose(server) => Some(server),
            _ => None,
        }
    }
}

const FORWARDER: &str = "dials a forwarder this image holds, which the next image would not";
const TRUST: &str = "ends only in this image, which removes the folder trust it was given";

const CLAUDE_CODE: HarnessFacts = HarnessFacts {
    name: "claude-code",
    program: "claude",
    usage: Some(UsageReader::Claude),
    needs_model_server: false,
    takes_tools: true,
    runs_guard_hooks: true,
    no_after_command: None,
    sessions: SessionIds::Kelpie,
    writing_tool: Some("Write"),
    reads_claude_files: true,
    ends_at_tool: true,
    drives_a_browser: true,
    keeps_its_servers: false,
    reads_from_an_empty_folder: false,
    transcript: Some(Format::Claude),
    counts_calls: true,
    runs_claude_roles: true,
    holds_handover_at_attach: None,
};

const PI: HarnessFacts = HarnessFacts {
    name: "pi",
    program: "pi",
    usage: Some(UsageReader::None),
    needs_model_server: true,
    takes_tools: false,
    runs_guard_hooks: false,
    no_after_command: None,
    sessions: SessionIds::Kelpie,
    writing_tool: Some("write"),
    reads_claude_files: false,
    ends_at_tool: true,
    drives_a_browser: true,
    keeps_its_servers: false,
    reads_from_an_empty_folder: false,
    transcript: Some(Format::Pi),
    counts_calls: true,
    runs_claude_roles: false,
    holds_handover_at_attach: Some(FORWARDER),
};

const CODEX: HarnessFacts = HarnessFacts {
    name: "codex",
    program: "codex",
    usage: Some(UsageReader::Codex),
    needs_model_server: false,
    takes_tools: false,
    runs_guard_hooks: false,
    no_after_command: None,
    sessions: SessionIds::Cli,
    writing_tool: Some("apply_patch"),
    reads_claude_files: false,
    ends_at_tool: false,
    drives_a_browser: true,
    keeps_its_servers: false,
    reads_from_an_empty_folder: true,
    transcript: Some(Format::Codex),
    counts_calls: false,
    runs_claude_roles: false,
    holds_handover_at_attach: Some(TRUST),
};

// The watch reads goose's stdout file, which each call empties, so no file
// holds a whole session and there is no transcript format.
const GOOSE: HarnessFacts = HarnessFacts {
    name: "goose",
    program: "goose",
    usage: Some(UsageReader::None),
    needs_model_server: true,
    takes_tools: false,
    runs_guard_hooks: false,
    no_after_command: Some(
        "goose's PostToolUse hook is not given a command's result, and the model \
         never reads what a hook prints",
    ),
    sessions: SessionIds::Kelpie,
    writing_tool: Some("write"),
    reads_claude_files: false,
    ends_at_tool: false,
    drives_a_browser: true,
    keeps_its_servers: true,
    reads_from_an_empty_folder: true,
    transcript: None,
    counts_calls: false,
    runs_claude_roles: false,
    holds_handover_at_attach: Some(FORWARDER),
};

#[cfg(any(test, feature = "test-support"))]
const STAND_IN: HarnessFacts = HarnessFacts {
    name: "stand-in",
    program: "stand-in",
    usage: None,
    needs_model_server: false,
    takes_tools: false,
    runs_guard_hooks: true,
    no_after_command: None,
    sessions: SessionIds::Kelpie,
    writing_tool: None,
    reads_claude_files: true,
    ends_at_tool: true,
    drives_a_browser: true,
    keeps_its_servers: false,
    reads_from_an_empty_folder: false,
    transcript: Some(Format::Claude),
    counts_calls: false,
    runs_claude_roles: false,
    holds_handover_at_attach: None,
};
