use std::path::Path;

use super::*;
use crate::ports::{CodeGraph, Role, Session, Tools, Usage};
use crate::settings::{ContextSize, EndpointUrl, ModelHost, ModelServer};
use crate::test::{RecordedSink, short_tempdir};

// A pi 1.1.0 session given the code graph, whose model called the graph's `list_projects` and
// whose lines hold no system message of its tools
const PI: &str = include_str!("../../../fixtures/pi-session-mcp.jsonl");
// A pi 1.1.0 worker session given the code graph, recorded after #949, whose system message lists
// the graph's tools in `toolsAdded`
const PI_CONNECTED: &str = include_str!("../../../fixtures/pi-session-mcp-connected.jsonl");
const SESSION: &str = "8b2d6e41-0c3f-4a97-b5e8-1d7f9a2c4e60";

// pi, as far as its session file goes: each call appends its lines to the file pi names
// `<time>_<session>.jsonl` in its sessions folder.
#[derive(Debug)]
struct Pi {
    sessions: PathBuf,
    text: &'static str,
}

impl Agents for Pi {
    fn prepare(&self, _call: &AgentCall) -> Result<(), AgentError> {
        Ok(())
    }

    fn run(&self, call: &AgentCall, _ending: &Ending) -> Result<AgentReply, AgentError> {
        let file = (self.sessions).join(format!("2026-10-15T19-59-36-998Z_{SESSION}.jsonl"));
        let before = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::write(&file, before + self.text).unwrap();
        Ok(AgentReply {
            session_id: call.session.id().clone(),
            text: "done".into(),
            usage: Usage::default(),
            session_cost: None,
            context: None,
            compactions: 0,
        })
    }

    fn transcript(&self, _call: &AgentCall) -> Option<Transcript> {
        Some(Transcript {
            folder: self.sessions.clone(),
            threads: None,
        })
    }
}

// A worker's pi call in `folder`, given the code graph
fn pi_call(folder: &Path) -> AgentCall {
    let server = ModelServer {
        host: ModelHost::Url(EndpointUrl::try_from("http://127.0.0.1:8080".to_owned()).unwrap()),
        context: ContextSize::try_from(65_536).unwrap(),
    };
    let mut call = AgentCall::new(
        Role::Worker,
        Tools::Work,
        AgentHarness::Pi(server),
        "qwen",
        crate::settings::Effort::Medium,
        Session::Resume(SessionId(SESSION.into())),
        folder.join("worktree"),
        "go on",
    );
    call.issue = 7;
    call.reach.code_graph = Some(CodeGraph {
        binary: folder.join("codebase-memory-mcp"),
        index: folder.join("graph"),
        runtime: folder.join("cbm"),
    });
    call
}

fn counted_over(sessions: PathBuf, text: &'static str) -> (TranscriptCount, RecordedSink) {
    let sink = RecordedSink::default();
    let counted = TranscriptCount {
        agents: Arc::new(Pi { sessions, text }),
        notes: Notes::default(),
        metrics: Arc::new(Recorder::new(Arc::new(sink.clone()))),
    };
    (counted, sink)
}

#[test]
fn each_pi_call_counts_its_own_tool_calls_from_its_session_file() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let (counted, sink) = counted_over(sessions, PI);
    let call = pi_call(folder.path());
    let graph = [("server", "codebase-memory-mcp")];

    counted.run(&call, &Ending::default()).unwrap();
    let tool_calls = sink.value("kelpie_mcp_tool_calls_total", &graph);
    assert_eq!(
        tool_calls,
        Some(1.0),
        "mcp__codebase_memory_mcp__ is the graph's"
    );
    counted.run(&call, &Ending::default()).unwrap();
    let tool_calls = sink.value("kelpie_mcp_tool_calls_total", &graph);
    assert_eq!(
        tool_calls,
        Some(2.0),
        "the second call's own, not the first's again"
    );
}

#[test]
fn a_pi_call_counts_a_server_whose_tools_its_session_loaded_connected() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let (counted, sink) = counted_over(sessions, PI_CONNECTED);
    let call = pi_call(folder.path());
    let connected = [("outcome", "connected"), ("server", "codebase-memory-mcp")];
    let failed = [("outcome", "failed"), ("server", "codebase-memory-mcp")];

    counted.run(&call, &Ending::default()).unwrap();
    assert_eq!(
        sink.value("kelpie_mcp_servers_total", &connected),
        Some(1.0)
    );
    assert_eq!(sink.value("kelpie_mcp_servers_total", &failed), None);
    assert!(counted.notes.standing_lines().is_empty());

    counted.run(&call, &Ending::default()).unwrap();
    assert_eq!(
        sink.value("kelpie_mcp_servers_total", &connected),
        Some(2.0),
        "the session loaded the tools once, and each call was given the server"
    );
}

#[test]
fn a_pi_call_counts_a_server_whose_tools_its_session_never_loaded_failed_and_notes_it_once() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let (counted, sink) = counted_over(sessions, PI);
    let call = pi_call(folder.path());
    let failed = [("outcome", "failed"), ("server", "codebase-memory-mcp")];

    counted.run(&call, &Ending::default()).unwrap();
    assert_eq!(sink.value("kelpie_mcp_servers_total", &failed), Some(1.0));
    let standing = counted.notes.standing_lines();
    assert_eq!(standing.len(), 1, "{standing:#?}");
    assert!(standing[0].contains("issue #7's worker"), "{}", standing[0]);
    assert!(
        standing[0].contains("its tools never loaded"),
        "pi records no reason: {}",
        standing[0]
    );

    counted.run(&call, &Ending::default()).unwrap();
    assert_eq!(
        sink.value("kelpie_mcp_servers_total", &failed),
        Some(2.0),
        "each call was given the server"
    );
    assert_eq!(counted.notes.standing_lines(), standing, "noted once");
}

#[test]
fn each_pi_call_counts_its_own_requests_from_its_session_file() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let (counted, sink) = counted_over(sessions, PI);
    let call = pi_call(folder.path());
    let small = [("harness", "pi"), ("le", "50000"), ("role", "worker")];

    counted.run(&call, &Ending::default()).unwrap();
    let requests = sink.value("kelpie_request_context_tokens", &small);
    assert_eq!(requests, Some(2.0), "the recorded session's two requests");
    counted.run(&call, &Ending::default()).unwrap();
    let requests = sink.value("kelpie_request_context_tokens", &small);
    assert_eq!(
        requests,
        Some(4.0),
        "the second call's two, not the first's again"
    );
    let start = [("after", "start"), ("harness", "pi"), ("role", "worker")];
    assert_eq!(
        sink.value("kelpie_cache_write_tokens_total", &start),
        Some(0.0),
        "the recorded session wrote nothing to a cache"
    );
}
