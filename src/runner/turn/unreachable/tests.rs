//! A turn whose model host cannot be reached waits and is tried again,
//! driven through the runner's scripted agent

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use super::super::tests::{usage, with_issue_7};
use crate::ports::{AgentError, Cost};
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Hold, Rig, Scripted};
use crate::usage::{FILE, read};

// Seconds since the first failure past which the wait parks the work item
const LIMIT: u64 = 30 * 60;

fn down(detail: &str) -> Scripted {
    Scripted::Fail(AgentError::Unreachable(Harness::ClaudeCode, detail.into()))
}

fn rulings(rig: &Rig, runner: &Mutex<Runner>) -> usize {
    rig.ask(runner, "status", None)["rulings"]
        .as_array()
        .map_or(0, Vec::len)
}

fn item(rig: &Rig, runner: &Mutex<Runner>) -> Value {
    rig.ask(runner, "status", None)["work_item"].clone()
}

#[test]
fn a_502_then_a_reply_ends_the_turn_with_no_ruling() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude.script([
        down(r#"502 "unreachable""#),
        Scripted::Reply(usage(1), Cost(7)),
    ]);
    assert_eq!(step(&runner).unwrap(), None, "the wait is not a report");
    assert_eq!(rulings(&rig, &runner), 0);

    rig.clock.advance(29);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.claude.calls().len(), 1, "it waits its 30 seconds first");

    rig.clock.advance(1);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { issue: 7, .. })
    ));
    assert_eq!(rig.claude.calls().len(), 2);
    assert_eq!(rulings(&rig, &runner), 0);
    assert_eq!(item(&rig, &runner).get("waiting"), None);
}

#[test]
fn the_retry_resumes_the_same_session() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude
        .script([down("503"), Scripted::Reply(usage(1), Cost(7))]);
    step(&runner).unwrap();
    rig.clock.advance(30);
    step(&runner).unwrap();
    let [first, retried] = rig.claude.calls().try_into().unwrap();
    assert_eq!(retried.session.id(), first.session.id());
    assert_eq!(retried.prompt, first.prompt);
    assert_eq!(
        item(&rig, &runner)["session"],
        json!(first.session.id().0),
        "the item keeps its session"
    );
}

#[test]
fn the_wait_grows_from_30_seconds_to_5_minutes() {
    let (rig, runner) = with_issue_7("acme");
    let failures: Vec<_> = (1..=7).map(|n| down(&format!("down {n}"))).collect();
    rig.claude.script(failures);
    step(&runner).unwrap();
    // The 30, 60, 120, 240 and 300 seconds after the 1st to 5th failure.
    for (wait, calls) in [(30, 2), (60, 3), (120, 4), (240, 5), (300, 6), (300, 7)] {
        rig.clock.advance(wait - 1);
        step(&runner).unwrap();
        assert_eq!(rig.claude.calls().len(), calls - 1, "{wait}s early");
        rig.clock.advance(1);
        step(&runner).unwrap();
        assert_eq!(rig.claude.calls().len(), calls, "{wait}s");
    }
}

#[test]
fn a_host_that_stays_down_past_the_limit_raises_one_turn_failed_ruling_with_the_last_error() {
    let (rig, runner) = with_issue_7("acme");
    let failures: Vec<_> = (1..=20)
        .map(|n| down(&format!("502 attempt {n}")))
        .collect();
    rig.claude.script(failures);
    let mut report = step(&runner).unwrap();
    let mut waited = 0;
    while report.is_none() && waited < 2 * LIMIT {
        rig.clock.advance(30);
        waited += 30;
        report = step(&runner).unwrap();
        assert!(
            report.is_none() || waited >= LIMIT,
            "parked after {waited}s: {report:?}"
        );
    }
    let Some(StepReport::Failed {
        issue, question, ..
    }) = report
    else {
        panic!("the host stayed down and nothing was raised: {report:?}");
    };
    assert_eq!(issue, 7);
    // The first retry at or after the limit, a pause of 5 minutes at the most.
    assert!((LIMIT..LIMIT + 300).contains(&waited), "{waited}s");
    let calls = rig.claude.calls().len();
    assert!(
        question.contains(&format!("502 attempt {calls}")),
        "the last error ({calls} calls): {question}"
    );
    assert_eq!(rulings(&rig, &runner), 1);
    assert_eq!(
        item(&rig, &runner)["turn"]["state"],
        "failed",
        "the turn is parked as a failed one is"
    );

    rig.clock.advance(3600);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.claude.calls().len(), calls, "no call after it parked");
    assert_eq!(rulings(&rig, &runner), 1);
}

#[test]
fn an_error_that_is_not_about_reaching_the_engine_still_parks_at_once() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude.script([Scripted::Fail(AgentError::Failed(
        Harness::ClaudeCode,
        r#"400 "bad request""#.into(),
    ))]);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Failed { issue: 7, .. })
    ));
    assert_eq!(rulings(&rig, &runner), 1);
    rig.clock.advance(3600);
    step(&runner).unwrap();
    assert_eq!(rig.claude.calls().len(), 1);
}

#[test]
fn the_waiting_mark_says_unreachable_and_since_when() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude
        .script([down("502"), Scripted::Reply(usage(1), Cost(7))]);
    step(&runner).unwrap();
    rig.clock.advance(20);
    step(&runner).unwrap();
    assert_eq!(
        item(&rig, &runner)["waiting"],
        json!({ "since": Rig::EPOCH, "why": "unreachable" })
    );
    let board = std::fs::read_to_string(&rig.paths().board).unwrap();
    assert!(
        board.contains("for its model host, which could not be reached\n"),
        "{board}"
    );
    assert!(board.contains("Waiting for its model since "), "{board}");
}

#[test]
fn a_retry_in_flight_sets_no_look_at_the_pause_that_has_passed() {
    let (rig, runner) = with_issue_7("acme");
    let hold = Hold::default();
    rig.claude
        .script([down("502"), Scripted::Hold(hold.clone())]);
    step(&runner).unwrap();
    assert_eq!(
        runner.lock().unwrap().next_look(),
        Some(Duration::from_secs(30)),
        "the pause is what the runner waits for"
    );
    rig.clock.advance(30);
    std::thread::scope(|scope| {
        let retry = scope.spawn(|| step(&runner));
        assert!(hold.entered(Duration::from_secs(10)), "no retry began");
        let look = runner.lock().unwrap().next_look();
        assert!(
            look.is_none_or(|look| look > Duration::from_secs(1)),
            "a look at once while the retry runs: {look:?}"
        );
        hold.release();
        retry.join().unwrap().unwrap();
    });
}

#[test]
fn a_retry_writes_the_workers_github_login_again() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    assert_eq!(rig.ask(&runner, "add", Some("7"))["work_item"]["issue"], 7);
    rig.claude
        .script([down("502"), Scripted::Reply(usage(1), Cost(7))]);
    step(&runner).unwrap();
    let hosts = rig.build_7().join("gh/hosts.yml");
    let first = std::fs::read_to_string(&hosts).unwrap();
    assert!(first.contains("ghs_test"), "{first}");

    // What the login holds by the time the host is back.
    std::fs::write(&hosts, "a login that ran out\n").unwrap();
    rig.clock.advance(30);
    step(&runner).unwrap();
    assert_eq!(rig.claude.calls().len(), 2);
    let again = std::fs::read_to_string(&hosts).unwrap();
    assert!(again.contains("ghs_test"), "{again}");
}

#[test]
fn a_retry_that_ran_a_long_while_before_its_502_begins_a_new_wait() {
    let (rig, runner) = with_issue_7("acme");
    let hold = Hold::default();
    rig.claude.script([
        down("502 first"),
        Scripted::HoldThenFail(
            hold.clone(),
            AgentError::Unreachable(Harness::ClaudeCode, "502 second".into()),
        ),
        Scripted::Reply(usage(1), Cost(7)),
    ]);
    step(&runner).unwrap();
    rig.clock.advance(30);
    std::thread::scope(|scope| {
        let retry = scope.spawn(|| step(&runner));
        assert!(hold.entered(Duration::from_secs(10)), "no retry began");
        // The retry reached its host and worked past the limit.
        rig.clock.advance(LIMIT + 60);
        hold.release();
        assert_eq!(retry.join().unwrap().unwrap(), None, "it parked");
    });
    assert_eq!(rulings(&rig, &runner), 0);
    assert_eq!(
        item(&rig, &runner)["waiting"],
        json!({ "since": Rig::EPOCH + 30 + LIMIT + 60, "why": "unreachable" }),
        "the wait runs from the second failure"
    );
    rig.clock.advance(30);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { issue: 7, .. })
    ));
}

#[test]
fn a_call_that_could_not_reach_its_host_leaves_a_failed_line_in_the_ledger() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude.script([down("502")]);
    step(&runner).unwrap();
    let lines = read(&rig.paths().folder.join(FILE)).unwrap();
    let [line] = lines.as_slice() else {
        panic!("one call, one line: {lines:#?}")
    };
    let line = serde_json::to_value(line).unwrap();
    assert_eq!(
        (line["role"].as_str(), line["ended"].as_str()),
        (Some("worker"), Some("failed"))
    );
}

#[test]
fn a_restart_begins_a_new_wait() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude
        .script([down("502"), Scripted::Reply(usage(1), Cost(7))]);
    step(&runner).unwrap();
    drop(runner);

    let runner = rig.open().unwrap();
    assert_eq!(item(&rig, &runner).get("waiting"), None);
    // The turn is still marked running, so the restart resumes its session
    // at once instead of waiting out a pause it no longer remembers.
    step(&runner).unwrap();
    let [first, resumed] = rig.claude.calls().try_into().unwrap();
    assert_eq!(resumed.session.id(), first.session.id());
    assert_eq!(rulings(&rig, &runner), 0);
}
