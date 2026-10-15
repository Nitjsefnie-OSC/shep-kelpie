use std::sync::{Arc, Mutex};

use super::*;
use crate::ports::{CLEAN, Cost, Usage};
use crate::request_use::{After, Request};
use crate::runner::{CHECKS_SETTLE, Runner, StepReport, step};
use crate::test::{RecordedSink, Rig, Scripted, write_script};
use crate::usage::Line;

const COUNTER: Family = Family {
    name: "kelpie_test_total",
    kind: SeriesKind::Counter,
    help: "A counter.",
};
const GAUGE: Family = Family {
    name: "kelpie_test",
    kind: SeriesKind::Gauge,
    help: "A gauge.",
};

fn recorder() -> (Recorder, RecordedSink) {
    let sink = RecordedSink::default();
    (Recorder::new(Arc::new(sink.clone())), sink)
}

fn label(value: &str) -> Vec<Label> {
    vec![("role", value.to_owned())]
}

#[test]
fn a_counter_sends_its_whole_total_so_a_dropped_sample_costs_nothing() {
    let (recorder, sink) = recorder();
    recorder.add(&COUNTER, label("worker"), 2.0);
    recorder.add(&COUNTER, label("worker"), 3.0);
    let values: Vec<_> = sink.sent().iter().map(|s| s.value).collect();
    assert_eq!(values, [2.0, 5.0]);
    assert_eq!(
        sink.value("kelpie_test_total", &[("role", "worker")]),
        Some(5.0)
    );
}

#[test]
fn labels_given_in_any_order_are_one_series() {
    let (recorder, sink) = recorder();
    let a = vec![("role", "worker".to_owned()), ("harness", "pi".to_owned())];
    let b = vec![("harness", "pi".to_owned()), ("role", "worker".to_owned())];
    recorder.add(&COUNTER, a, 1.0);
    recorder.add(&COUNTER, b, 1.0);
    let pair = [("harness", "pi"), ("role", "worker")];
    assert_eq!(sink.value("kelpie_test_total", &pair), Some(2.0));
}

#[test]
fn a_gauge_is_sent_when_new_or_changed_and_not_when_set_to_what_it_holds() {
    let (recorder, sink) = recorder();
    recorder.set(&GAUGE, label("a"), 0.0);
    recorder.set(&GAUGE, label("a"), 0.0);
    recorder.set(&GAUGE, label("a"), 3.0);
    recorder.set(&GAUGE, label("a"), 3.0);
    let values: Vec<_> = sink.sent().iter().map(|s| s.value).collect();
    assert_eq!(values, [0.0, 3.0], "a new gauge at 0 is still sent once");
}

#[test]
fn every_series_goes_again_once_a_minute_for_a_metrics_dog_that_started_since() {
    let (recorder, sink) = recorder();
    recorder.add(&COUNTER, label("worker"), 1.0);
    recorder.set(&GAUGE, label("a"), 2.0);
    recorder.refresh(Timestamp(1_000));
    assert_eq!(sink.sent().len(), 4, "the first refresh sends both");
    recorder.refresh(Timestamp(1_059));
    assert_eq!(sink.sent().len(), 4, "not again within the minute");
    recorder.refresh(Timestamp(1_060));
    assert_eq!(sink.sent().len(), 6);
}

#[test]
fn a_new_series_past_the_cap_is_not_sent_and_is_counted() {
    let (recorder, sink) = recorder();
    for n in 0..SERIES_CAP {
        recorder.add(&COUNTER, label(&n.to_string()), 1.0);
    }
    recorder.add(&COUNTER, label("one more"), 1.0);
    recorder.add(&COUNTER, label("and another"), 1.0);
    assert_eq!(
        sink.value("kelpie_test_total", &[("role", "one more")]),
        None
    );
    assert_eq!(sink.value("kelpie_series_dropped_total", &[]), Some(2.0));
    recorder.add(&COUNTER, label("0"), 1.0);
    assert_eq!(
        sink.value("kelpie_test_total", &[("role", "0")]),
        Some(2.0),
        "a series already held still counts"
    );
}

// The last value of each `name` series whose labels hold `role`, keyed by its `type` label
fn by_type(sink: &RecordedSink, name: &str, role: &str) -> Vec<(String, f64)> {
    let mut last = std::collections::BTreeMap::new();
    for sample in sink.sent().into_iter().filter(|s| s.name == name) {
        if sample.labels.contains(&("role", role.into())) {
            let kind = sample.labels.iter().find(|(k, _)| *k == "type");
            last.insert(
                kind.map_or_else(String::new, |(_, v)| v.clone()),
                sample.value,
            );
        }
    }
    last.into_iter().collect()
}

#[test]
fn a_turn_s_tokens_and_cost_are_counted_by_role_and_type() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let usage = Usage {
        input: 10,
        cache_write: 200,
        cache_write_5m: 0,
        cache_read: 3_000,
        output: 40,
    };
    let cost = Cost::from_usd(0.25).unwrap();
    rig.claude.script([Scripted::Reply(usage, cost)]);
    step(&runner).unwrap();
    let tokens = by_type(&rig.samples, "kelpie_tokens_total", "worker");
    let expected = [
        ("cache_read", 3_000.0),
        ("cache_write", 200.0),
        ("input", 10.0),
        ("output", 40.0),
    ];
    let expected: Vec<_> = expected
        .iter()
        .map(|(k, v)| ((*k).to_owned(), *v))
        .collect();
    assert_eq!(tokens, expected);
    let spent = by_type(&rig.samples, "kelpie_cost_usd_total", "worker");
    assert_eq!(spent, [(String::new(), 0.25)]);
}

fn turn_on_the_code_graph(rig: &Rig, script: Option<&str>) {
    let off = "[app.dogs.kelpie.code_graph]\nenabled = false";
    rig.edit_settings(|s| s.replace(off, "[app.dogs.kelpie.code_graph]\nenabled = true"));
    if let Some(script) = script {
        let binary = rig.paths().tools.code_graph();
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        write_script(&binary, script);
    }
}

#[test]
fn a_work_item_s_calls_spend_and_finish_reach_the_metrics_endpoint() {
    let (rig, runner, _) = Rig::parked_set("shep", |_| {});
    let calls = rig.samples.total("kelpie_calls_total");
    assert!(
        calls >= 2.0,
        "the worker's turn and a reviewer's round: {calls}"
    );
    let worker_turn = rig.samples.sent().into_iter().find(|s| {
        s.name == "kelpie_calls_total"
            && s.labels.contains(&("role", "worker".into()))
            && s.labels.contains(&("kind", "turn".into()))
    });
    let worker_turn = worker_turn.expect("the worker's turn is counted");
    assert!(
        worker_turn.labels.contains(&("ended", "answered".into())),
        "{worker_turn:?}"
    );
    assert_eq!(
        rig.samples
            .value("kelpie_work_items", &[("phase", "ruling")]),
        Some(1.0)
    );
    assert_eq!(
        rig.samples
            .value("kelpie_work_items", &[("phase", "implement")]),
        Some(0.0)
    );

    rig.ask(&runner, "rule", Some("71 yes"));
    step(&runner).unwrap(); // marked ready
    rig.clock.advance(CHECKS_SETTLE);
    let finished = step(&runner).unwrap();
    assert!(matches!(
        finished,
        Some(StepReport::Finished { issue: 7, .. })
    ));
    let merged = [("outcome", "merged")];
    assert_eq!(
        rig.samples
            .value("kelpie_work_items_finished_total", &merged),
        Some(1.0)
    );
    assert_eq!(
        rig.samples
            .value("kelpie_work_items", &[("phase", "ruling")]),
        Some(0.0)
    );
}

#[test]
fn an_enabled_code_graph_that_never_indexes_shows_as_on_with_no_ok_run() {
    let failing = "#!/bin/sh\necho 'endpoint could not be created' >&2\nexit 1\n";
    let (rig, _runner, _) =
        Rig::parked_set("shep", |rig| turn_on_the_code_graph(rig, Some(failing)));
    let graph = [("feature", "code_graph")];
    assert_eq!(
        rig.samples.value("kelpie_feature_enabled", &graph),
        Some(1.0)
    );
    let failed = [("feature", "code_graph"), ("outcome", "failed")];
    assert!(rig.samples.value("kelpie_feature_runs_total", &failed) >= Some(1.0));
    let ok = [("feature", "code_graph"), ("outcome", "ok")];
    assert_eq!(rig.samples.value("kelpie_feature_runs_total", &ok), None);
    let browser = [("feature", "browser")];
    assert_eq!(
        rig.samples.value("kelpie_feature_enabled", &browser),
        Some(0.0)
    );
}

#[test]
fn a_code_graph_whose_server_is_not_installed_counts_as_skipped() {
    let (rig, _runner, _) = Rig::parked_set("shep", |rig| turn_on_the_code_graph(rig, None));
    let skipped = [("feature", "code_graph"), ("outcome", "skipped")];
    assert!(rig.samples.value("kelpie_feature_runs_total", &skipped) >= Some(1.0));
}

#[test]
fn an_indexed_code_graph_counts_as_ok() {
    let indexer = "#!/bin/sh\nexit 0\n";
    let (rig, _runner, _) =
        Rig::parked_set("shep", |rig| turn_on_the_code_graph(rig, Some(indexer)));
    let ok = [("feature", "code_graph"), ("outcome", "ok")];
    assert!(rig.samples.value("kelpie_feature_runs_total", &ok) >= Some(1.0));
}

// A worker's session whose code graph failed to connect in its first call, which the last of its
// four calls found connected. From Claude Code 2.1.300.
const FAILED: &str = include_str!("../../fixtures/claude-transcript-mcp-failed.jsonl");

// The recorded session's first call, as its transcript stood when it ended
fn failed_call() -> String {
    FAILED
        .lines()
        .take(3)
        .map(|line| format!("{line}\n"))
        .collect()
}

// Issue 7's worker turn and a Claude reviewer's round, with the code graph, each call writing its
// transcript as given
fn worker_and_reviewer(worker: String, reviewer: String) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    turn_on_the_code_graph(&rig, Some("#!/bin/sh\nexit 0\n"));
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::Transcribed(worker, Box::new(Scripted::Push("work.txt", "work\n"))),
        Scripted::Transcribed(reviewer, Box::new(Scripted::Text(CLEAN))),
    ]);
    step(&runner).unwrap(); // the worker's first turn
    step(&runner).unwrap(); // round 1, qwen: clean by default
    step(&runner).unwrap(); // round 2, claude: scripted clean above
    (rig, runner)
}

#[test]
fn a_server_that_failed_to_connect_is_counted_failed_and_noted_once_while_it_stands() {
    let (rig, runner) = worker_and_reviewer(failed_call(), failed_call());
    let failed = [("outcome", "failed"), ("server", "codebase-memory-mcp")];
    assert_eq!(
        rig.samples.value("kelpie_mcp_servers_total", &failed),
        Some(2.0)
    );
    let graph = [("server", "codebase-memory-mcp")];
    assert_eq!(
        rig.samples.value("kelpie_mcp_tool_calls_total", &graph),
        Some(0.0),
        "a server given and never called reads 0"
    );
    let notes = runner.lock().unwrap().take_notes();
    let said: Vec<_> = (notes.iter())
        .filter(|n| n.contains("`codebase-memory-mcp`"))
        .collect();
    assert_eq!(said.len(), 1, "{notes:#?}");
    assert!(said[0].contains("issue #7's worker"), "{}", said[0]);
    assert!(said[0].contains("CONNECTION_CLOSED"), "{}", said[0]);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["standing"], serde_json::json!([said[0]]));
}

#[test]
fn a_failed_server_that_connects_in_a_later_call_ends_its_standing_note() {
    let (rig, runner) = worker_and_reviewer(failed_call(), FAILED.to_owned());
    let outcome = |outcome| {
        let labels = [("outcome", outcome), ("server", "codebase-memory-mcp")];
        rig.samples.value("kelpie_mcp_servers_total", &labels)
    };
    assert_eq!(outcome("failed"), Some(1.0));
    assert_eq!(outcome("connected"), Some(1.0));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["standing"], serde_json::Value::Null, "none stands");
}

fn bucket(rig: &Rig, role: &str, le: &str) -> Option<f64> {
    let labels = [("harness", "claude-code"), ("le", le), ("role", role)];
    rig.samples.value("kelpie_request_context_tokens", &labels)
}

fn cache_writes(rig: &Rig, role: &str, after: &str) -> Option<f64> {
    let labels = [("after", after), ("harness", "claude-code"), ("role", role)];
    rig.samples
        .value("kelpie_cache_write_tokens_total", &labels)
}

// The worker's turn writes the recorded session's first call to its transcript, and the reviewer's
// round the whole session: three requests an hour or more apart, with contexts of 18,576, 69,169
// and 76,220 tokens and cache writes of 13,152, 63,745 and 70,796.
#[test]
fn a_call_s_requests_are_counted_by_context_size_and_what_came_before() {
    let (rig, _runner) = worker_and_reviewer(failed_call(), FAILED.to_owned());
    let buckets = |role| {
        ["25000", "50000", "100000", "150000", "200000", "+Inf"].map(|le| bucket(&rig, role, le))
    };
    assert_eq!(buckets("worker"), [Some(1.0); 6], "one request of 18,576");
    assert_eq!(
        buckets("reviewer"),
        [1.0, 1.0, 3.0, 3.0, 3.0, 3.0].map(Some),
        "18,576, then 69,169 and 76,220: cumulative"
    );
    let writes = |role| ["start", "gap", "growth"].map(|after| cache_writes(&rig, role, after));
    assert_eq!(writes("worker"), [Some(13_152.0), Some(0.0), Some(0.0)]);
    assert_eq!(
        writes("reviewer"),
        [Some(13_152.0), Some(134_541.0), Some(0.0)],
        "the two requests an hour apart wrote 63,745 and 70,796 after a gap"
    );
}

// A call line of `role` and `kind` on `harness`, which ended `ended`
fn call_line(role: &str, kind: &str, harness: &str, ended: &str) -> Line {
    let usage = serde_json::json!({
        "input": 1, "cache_write": 1, "cache_read": 1, "output": 1,
    });
    Line::Call(
        serde_json::from_value(serde_json::json!({
            "at": 1, "issue": 7, "role": role, "kind": kind, "harness": harness,
            "model": "model", "usage": usage, "units": 1, "cost_usd": 0.5,
            "seconds": 3, "ended": ended,
        }))
        .unwrap(),
    )
}

#[test]
fn a_runner_with_every_role_and_harness_in_use_stays_under_the_cap() {
    let (recorder, sink) = recorder();
    // Each role on a harness it might have, with its kinds of call and two ways each ended. The
    // reviewer runs on three; only Claude Code's and pi's calls have requests.
    let roles = [
        ("worker", "claude-code", &["turn", "compact", "vote"][..]),
        ("reviewer", "claude-code", &["review"][..]),
        ("reviewer", "pi", &["review"][..]),
        ("reviewer", "local", &["local-round"][..]),
        ("issue-writer", "pi", &["issues"][..]),
        ("pm", "claude-code", &["wake", "compact", "retro"][..]),
        ("advisor", "codex", &["vote"][..]),
        ("researcher", "goose", &["vote"][..]),
        ("representative", "pi", &["post"][..]),
        ("triager", "claude-code", &["triage"][..]),
    ];
    for (role, harness, kinds) in roles {
        for kind in kinds {
            for ended in ["answered", "failed"] {
                recorder.line(&call_line(role, kind, harness, ended));
            }
        }
        if harness == "claude-code" || harness == "pi" {
            let request = Request {
                context: 30_000,
                cache_write: 1,
                after: After::Gap,
            };
            recorder.request(role, harness, &request);
        }
    }
    for outcome in ["ok", "failed", "skipped", "ended"] {
        recorder.feature_ran(Feature::CodeGraph, outcome);
    }
    for server in ["codebase-memory-mcp", "playwright", "repo"] {
        recorder.mcp_tool_calls(server, 1);
        recorder.mcp_server(server, true);
        recorder.mcp_server(server, false);
    }
    // The series set from the state and the settings, and the finished work items', which these
    // lines do not drive: six phases, three features, and nine of the finished line's.
    let not_driven = 6 + 3 + 9;
    let held: std::collections::BTreeSet<_> = (sink.sent().iter())
        .map(|s| (s.name, s.labels.clone()))
        .collect();
    let requests = held
        .iter()
        .filter(|(name, _)| {
            [
                "kelpie_request_context_tokens",
                "kelpie_cache_write_tokens_total",
            ]
            .contains(name)
        })
        .count();
    eprintln!(
        "series: {} and {not_driven} more, {requests} of them requests",
        held.len()
    );
    assert_eq!(
        requests,
        7 * 9,
        "six buckets and three writes for each pair"
    );
    assert!(
        held.len() + not_driven < SERIES_CAP,
        "{}",
        held.len() + not_driven
    );
    assert_eq!(sink.value("kelpie_series_dropped_total", &[]), None);
}
