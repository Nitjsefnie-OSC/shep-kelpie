use std::sync::Arc;

use super::*;
use crate::ports::{Cost, Usage};
use crate::runner::{CHECKS_SETTLE, StepReport, step};
use crate::test::{RecordedSink, Rig, Scripted, write_script};

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
