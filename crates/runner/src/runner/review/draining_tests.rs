//! A review round that a `drain` lands on while it reads the issue it reviews
//!
//! A round checks that the runner drains, then reads the issue it reviews the
//! criteria from, with the runner's lock held. A `drain` answered from the
//! posted copy in that read sets its flag there and waits for nothing, so the
//! round checks it again before it books the reviewer's call.

use std::sync::{Arc, Mutex};

use super::*;
use crate::runner::answer;
use crate::runner::flight::{Pass, advance};
use crate::runner::step;
use crate::test::{CALL_PATIENCE, Rig, Scripted};

// A work item with its pull request open and its head pushed, at its first round
fn at_round_1(project: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn a_drain_that_lands_while_the_issue_is_read_for_a_round_starts_no_call() {
    let (rig, runner) = at_round_1("koji");
    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let read = rig.forge.hold_next_issue_read();

    std::thread::scope(|scope| {
        let pass = scope.spawn(|| advance(&runner));
        assert!(
            read.waiting(CALL_PATIENCE),
            "the round never read the issue it reviews"
        );
        let asking = {
            let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
            scope.spawn(move || answer(&runner, &postings, "drain", Some("pause")))
        };
        let reply: serde_json::Value =
            serde_json::from_str(&asking.join().unwrap()).expect("a JSON reply");
        assert!(
            postings.draining(),
            "the drain did not hold new calls back: {reply}"
        );
        drop(read);
        let pass = pass.join().expect("the pass thread");
        assert!(
            matches!(pass, Ok(Pass::Idle)),
            "the pass started a review past the drain: {pass:?}"
        );
        assert_eq!(rig.reviewer.seen(), [], "the reviewer ran anyway");
        assert_eq!(
            rig.ask(&runner, "status", None)["work_item"]["phase"]["round"],
            1,
            "the round was booked past the drain"
        );
    });
}

// The same for a bot's round: its activity is read before the summon, and a
// drain that lands in that read stops the summon before the lease is asked.
#[test]
fn a_drain_that_lands_while_a_bots_activity_is_read_starts_no_call() {
    let (rig, runner, _head) = crate::runner::coderabbit::tests::reviewed_by_qwen("koji");
    assert!(matches!(
        step(&runner).unwrap(),
        Some(crate::runner::StepReport::MarkedReady { .. })
    ));
    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let read = rig.forge.hold_next_activity_read();

    std::thread::scope(|scope| {
        let pass = scope.spawn(|| advance(&runner));
        assert!(
            read.waiting(CALL_PATIENCE),
            "the round never read the bot's activity"
        );
        let asking = {
            let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
            scope.spawn(move || answer(&runner, &postings, "drain", Some("pause")))
        };
        let reply: serde_json::Value =
            serde_json::from_str(&asking.join().unwrap()).expect("a JSON reply");
        assert!(
            postings.draining(),
            "the drain did not hold new calls back: {reply}"
        );
        drop(read);
        let pass = pass.join().expect("the pass thread");
        assert!(
            matches!(pass, Ok(Pass::Idle)),
            "the pass summoned the bot past the drain: {pass:?}"
        );
        assert_eq!(rig.forge.coderabbit.label_log(), [], "the bot was summoned");
        assert_eq!(rig.leases.told(), [], "the bot's lease was asked for");
    });
}
