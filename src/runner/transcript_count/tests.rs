use std::path::Path;

use super::*;
use crate::ports::{CodeGraph, Role, Session, Tools, Usage};
use crate::settings::{ContextSize, EndpointUrl, ModelHost, ModelServer};
use crate::test::{RecordedSink, short_tempdir};

// A pi 1.1.0 session given the code graph, whose model called the graph's `list_projects`
const PI: &str = include_str!("../../../fixtures/pi-session-mcp.jsonl");
const SESSION: &str = "8b2d6e41-0c3f-4a97-b5e8-1d7f9a2c4e60";

// pi, as far as its session file goes: each call appends its lines to the file pi names
// `<time>_<session>.jsonl` in its sessions folder.
#[derive(Debug)]
struct Pi {
    sessions: PathBuf,
}

impl Agents for Pi {
    fn prepare(&self, _call: &AgentCall) -> Result<(), AgentError> {
        Ok(())
    }

    fn run(&self, call: &AgentCall, _ending: &Ending) -> Result<AgentReply, AgentError> {
        let file = (self.sessions).join(format!("2026-10-15T19-59-36-998Z_{SESSION}.jsonl"));
        let before = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::write(&file, before + PI).unwrap();
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
    call.reach.code_graph = Some(CodeGraph {
        binary: folder.join("codebase-memory-mcp"),
        index: folder.join("graph"),
        runtime: folder.join("cbm"),
    });
    call
}

#[test]
fn each_pi_call_counts_its_own_tool_calls_from_its_session_file_and_no_server() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let sink = RecordedSink::default();
    let counted = TranscriptCount {
        agents: Arc::new(Pi { sessions }),
        notes: Notes::default(),
        metrics: Arc::new(Recorder::new(Arc::new(sink.clone()))),
    };
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
    assert_eq!(sink.total("kelpie_mcp_servers_total"), 0.0);
}

#[test]
fn each_pi_call_counts_its_own_requests_from_its_session_file() {
    let folder = short_tempdir();
    let sessions = folder.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let sink = RecordedSink::default();
    let counted = TranscriptCount {
        agents: Arc::new(Pi { sessions }),
        notes: Notes::default(),
        metrics: Arc::new(Recorder::new(Arc::new(sink.clone()))),
    };
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
